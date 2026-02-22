use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use ssh_agent_lib::{
    agent::{self, Agent, ListeningSocket, Session},
    client,
    error::AgentError,
    proto::{extension::QueryResponse, Extension, Identity, SignRequest},
    ssh_key::{public::KeyData as PubKeyData, Signature},
};
use tokio::{
    net::UnixListener,
    sync::{Mutex, OwnedMutexGuard},
    time::timeout,
};

type KnownPubKeysMap = HashMap<PubKeyData, PathBuf>;
type KnownPubKeys = Arc<Mutex<KnownPubKeysMap>>;

/// Only the `request_identities`, `sign`, `add_identity`, `lock`, `unlock`, and `extension`
/// commands are implemented.
/// For `extension`, only the `session-bind@openssh.com` and `query` extensions are supported.
#[ssh_agent_lib::async_trait]
impl Session for MuxAgent {
    async fn request_identities(&mut self) -> Result<Vec<Identity>, AgentError> {
        log::trace!("incoming: request_identities");
        let mut known_keys = self.known_keys.clone().lock_owned().await;
        self.refresh_identities(&mut known_keys).await
    }

    async fn sign(&mut self, request: SignRequest) -> Result<Signature, AgentError> {
        let fingerprint = request.pubkey.fingerprint(Default::default());
        log::trace!("incoming: sign({})", &fingerprint);

        if let Some(agent_sock_path) = self.get_agent_sock_for_pubkey(&request.pubkey).await? {
            log::info!(
                "Requesting signature with key {} from upstream agent <{}>",
                &fingerprint,
                agent_sock_path.display()
            );

            let mut client = self.connect_upstream_agent(&agent_sock_path).await?;
            timeout(self.agent_timeout, client.sign(request))
                .await
                .map_err(|_| {
                    AgentError::Other(
                        format!(
                            "Sign request timed out on upstream agent: {}",
                            agent_sock_path.display()
                        )
                        .into(),
                    )
                })?
        } else {
            log::error!("No upstream agent found for public key {}", &fingerprint);
            log::trace!("Known keys:\n{:#?}", self.known_keys);
            Err(AgentError::Other(
                format!("No agent found for public key: {}", &fingerprint).into(),
            ))
        }
    }

    async fn extension(&mut self, request: Extension) -> Result<Option<Extension>, AgentError> {
        log::trace!("incoming: extension({})", request.name);
        match request.name.as_str() {
            "query" => Ok(Some(Extension::new_message(QueryResponse {
                extensions: ["session-bind@openssh.com"].map(String::from).to_vec(),
            })?)),
            "session-bind@openssh.com" => {
                let mut session_bind_suceeded = false;
                for sock_path in &self.socket_paths {
                    // Try extension on upstream agents; discard any upstream failures from agents
                    // that don't support the extension (but the default is Failure if there are no
                    // successful upstream responses)
                    let mut client = match self.connect_upstream_agent(sock_path).await {
                        Ok(c) => c,
                        Err(_) => continue,
                    };
                    let result = match timeout(self.agent_timeout, client.extension(request.clone())).await {
                        Ok(r) => r,
                        Err(_) => {
                            log::warn!(
                                "Extension request timed out on upstream agent: {}",
                                sock_path.display()
                            );
                            continue;
                        }
                    };
                    match result {
                        // Any agent succeeding is an overall success
                        Ok(v) => {
                            session_bind_suceeded = true;
                            if v.is_some() {
                                log::warn!("session-bind@openssh.com request succeeded on socket <{}>, but an invalid response was received", sock_path.display());
                            }
                        }
                        // Don't propagate upstream lack of extension support
                        Err(AgentError::Failure) => continue,
                        // Report but ignore any unexpected errors
                        Err(e) => {
                            log::error!("Unexpected error on socket <{}> when requesting session-bind@openssh.com extension: {}", sock_path.display(), e);
                            continue;
                        }
                    }
                }
                if session_bind_suceeded {
                    Ok(None)
                } else {
                    Err(AgentError::Failure)
                }
            }
            _ => Err(AgentError::Failure),
        }
    }

    async fn lock(&mut self, key: String) -> Result<(), AgentError> {
        log::trace!("incoming: lock");
        for sock_path in &self.socket_paths {
            let mut client = self.connect_upstream_agent(sock_path).await?;
            timeout(self.agent_timeout, client.lock(key.clone()))
                .await
                .map_err(|_| {
                    AgentError::Other(
                        format!(
                            "Lock request timed out on upstream agent: {}",
                            sock_path.display()
                        )
                        .into(),
                    )
                })??;
            log::info!(
                "Locked upstream agent <{}>",
                sock_path.display()
            );
        }
        Ok(())
    }

    async fn unlock(&mut self, key: String) -> Result<(), AgentError> {
        log::trace!("incoming: unlock");
        for sock_path in &self.socket_paths {
            let mut client = self.connect_upstream_agent(sock_path).await?;
            timeout(self.agent_timeout, client.unlock(key.clone()))
                .await
                .map_err(|_| {
                    AgentError::Other(
                        format!(
                            "Unlock request timed out on upstream agent: {}",
                            sock_path.display()
                        )
                        .into(),
                    )
                })??;
            log::info!(
                "Unlocked upstream agent <{}>",
                sock_path.display()
            );
        }
        Ok(())
    }

    async fn add_identity(
        &mut self,
        identity: ssh_agent_lib::proto::AddIdentity,
    ) -> Result<(), AgentError> {
        log::trace!("incoming: add_identity");

        if let Some(added_keys_sock) = &self.added_keys_sock {
            log::info!(
                "Forwarding add_identity request to upstream agent <{}>",
                added_keys_sock.display()
            );

            let mut client = self.connect_upstream_agent(added_keys_sock).await?;
            timeout(self.agent_timeout, client.add_identity(identity))
                .await
                .map_err(|_| {
                    AgentError::Other(
                        format!(
                            "Add identity request timed out on upstream agent: {}",
                            added_keys_sock.display()
                        )
                        .into(),
                    )
                })?
        } else {
            log::error!("add_identity requested but no added_keys socket configured");
            Err(AgentError::Failure)
        }
    }
}

#[derive(Clone)]
pub struct MuxAgent {
    socket_paths: Vec<PathBuf>,
    added_keys_sock: Option<PathBuf>,
    known_keys: KnownPubKeys,
    agent_timeout: Duration,
}

impl MuxAgent {
    /// Run a MuxAgent, listening for SSH agent protocol requests on `listen_sock`, forwarding
    /// requests to the specified paths in `agent_socks`
    pub async fn run<I, P>(
        listen_sock: impl AsRef<Path>,
        agent_socks: I,
        added_keys_sock: Option<PathBuf>,
        agent_timeout: Duration,
    ) -> Result<(), AgentError>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let listen_sock = listen_sock.as_ref();
        let socket_paths: Vec<_> = agent_socks
            .into_iter()
            .map(|p| p.as_ref().to_path_buf())
            .collect();
        if socket_paths.is_empty() {
            log::warn!("Mux agent running but no upstream agents configured");
        }
        log::info!(
            "Starting agent for {} upstream agents; listening on <{}>",
            socket_paths.len(),
            listen_sock.display()
        );
        log::debug!("Upstream agent sockets: {:?}", &socket_paths);
        if let Some(ref added_keys) = added_keys_sock {
            log::info!("add_identity requests will be forwarded to <{}>", added_keys.display());
        }

        let listen_sock = match SelfDeletingUnixListener::bind(listen_sock) {
            Ok(s) => s,
            err => {
                log::error!(
                    "Failed to open listening socket at {}",
                    listen_sock.display()
                );
                err?
            }
        };
        let this = Self {
            socket_paths,
            added_keys_sock,
            known_keys: Default::default(),
            agent_timeout,
        };
        agent::listen(listen_sock, this).await
    }

    async fn connect_upstream_agent(
        &self,
        sock_path: impl AsRef<Path>,
    ) -> Result<Box<dyn Session>, AgentError> {
        let sock_path = sock_path.as_ref();
        let stream = timeout(self.agent_timeout, tokio::net::UnixStream::connect(sock_path))
            .await
            .map_err(|_| {
                AgentError::Other(
                    format!(
                        "Connection to upstream agent timed out: {}",
                        sock_path.display()
                    )
                    .into(),
                )
            })?
            .map_err(AgentError::IO)?;
        let client = client::connect(stream.into_std()?.into()).map_err(|e| {
            AgentError::Other(
                format!(
                    "Failed to connect to agent at {}: {}",
                    sock_path.display(),
                    e
                )
                .into(),
            )
        })?;
        log::trace!(
            "Connected to upstream agent on socket: {}",
            sock_path.display()
        );
        Ok(client)
    }

    async fn get_agent_sock_for_pubkey(
        &mut self,
        pubkey: &PubKeyData,
    ) -> Result<Option<PathBuf>, AgentError> {
        // Refresh available identities if the public key isn't found;
        // hold lock for duration of signing operation
        let mut known_keys = self.known_keys.clone().lock_owned().await;
        if !known_keys.contains_key(pubkey) {
            log::debug!("Key not found, re-requesting keys from upstream agents");
            let _ = self.refresh_identities(&mut known_keys).await?;
        }
        let maybe_agent = known_keys.get(pubkey).cloned();
        Ok(maybe_agent)
    }

    // Factored out so that the known_keys lock can be held across a total request that includes a
    // refresh of keys from upstream agents
    async fn refresh_identities(
        &mut self,
        known_keys: &mut OwnedMutexGuard<KnownPubKeysMap>,
    ) -> Result<Vec<Identity>, AgentError> {
        let mut identities = vec![];
        known_keys.clear();

        log::debug!("Refreshing identities");
        for sock_path in &self.socket_paths {
            let mut client = match self.connect_upstream_agent(sock_path).await {
                Ok(c) => c,
                Err(_) => {
                    log::warn!(
                        "Ignoring missing upstream agent socket: {}",
                        sock_path.display()
                    );
                    continue;
                }
            };
            let agent_identities: Vec<Identity> = match timeout(
                self.agent_timeout,
                client.request_identities(),
            )
            .await
            {
                Ok(Ok(ids)) => ids,
                Ok(Err(e)) => {
                    log::warn!(
                        "Failed to request identities from upstream agent socket <{}>: {}",
                        sock_path.display(),
                        e
                    );
                    continue;
                }
                Err(_) => {
                    log::warn!(
                        "Request identities timed out on upstream agent: {}",
                        sock_path.display()
                    );
                    continue;
                }
            };
            {
                for id in &agent_identities {
                    known_keys.insert(id.pubkey.clone(), sock_path.clone());
                }
            }
            log::trace!(
                "Got {} identities from {}",
                agent_identities.len(),
                sock_path.display()
            );
            identities.extend(agent_identities);
        }

        Ok(identities)
    }
}

impl Agent<SelfDeletingUnixListener> for MuxAgent {
    #[doc = "Create new session object when a new socket is accepted."]
    fn new_session(
        &mut self,
        _socket: &<SelfDeletingUnixListener as ListeningSocket>::Stream,
    ) -> impl Session {
        self.clone()
    }
}

#[derive(Debug)]
/// A wrapper for UnixListener that keeps the socket path around so it can be deleted
struct SelfDeletingUnixListener {
    path: PathBuf,
    listener: UnixListener,
}

impl SelfDeletingUnixListener {
    fn bind(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        UnixListener::bind(&path).map(|listener| Self { path, listener })
    }
}

impl Drop for SelfDeletingUnixListener {
    fn drop(&mut self) {
        log::debug!("Cleaning up socket {}", self.path.display());
        let _ = std::fs::remove_file(&self.path);
    }
}

#[ssh_agent_lib::async_trait]
impl ListeningSocket for SelfDeletingUnixListener {
    type Stream = tokio::net::UnixStream;

    async fn accept(&mut self) -> std::io::Result<Self::Stream> {
        UnixListener::accept(&self.listener)
            .await
            .map(|(s, _addr)| s)
    }
}
