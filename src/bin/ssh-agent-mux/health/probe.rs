//! Protocol probes: connect to agent sockets over the SSH agent protocol
//! and count identities, for the "listen socket answers" and
//! "upstream <name> answers" health checks.

use std::{path::Path, time::Duration};

use ssh_agent_lib::client;

use crate::cli::Config;

/// Pre-resolved probe outcomes consumed by `emit_checks`. `upstreams`
/// is parallel to `config.agents` (config order); `None` entries are
/// agents disabled in config, whose probe was skipped.
pub struct ProbeReport {
    pub listen: Result<usize, String>,
    pub upstreams: Vec<Option<Result<usize, String>>>,
}

/// Probe the listen socket and every enabled upstream. The listen probe
/// runs concurrently with the upstream chain via `tokio::join!`;
/// upstreams are probed sequentially among themselves to keep config
/// ordering without pulling in a futures dependency for `join_all`.
pub async fn probe_all(config: &Config) -> ProbeReport {
    let timeout = Duration::from_secs(config.agent_timeout);
    let listen = probe_agent(&config.listen_path, timeout);
    let upstreams = async {
        let mut results = Vec::with_capacity(config.agents.len());
        for agent in &config.agents {
            let result = if agent.enabled {
                Some(probe_agent(&agent.socket_path, timeout).await)
            } else {
                None
            };
            results.push(result);
        }
        results
    };
    let (listen, upstreams) = tokio::join!(listen, upstreams);
    ProbeReport { listen, upstreams }
}

/// Connect to an agent socket and count its identities.
pub async fn probe_agent(path: &Path, timeout: Duration) -> Result<usize, String> {
    let fut = async {
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .map_err(|e| format!("connect {}: {e}", path.display()))?;
        let stream = stream.into_std().map_err(|e| e.to_string())?;
        let mut agent = client::connect(stream.into()).map_err(|e| e.to_string())?;
        let ids = agent
            .request_identities()
            .await
            .map_err(|e| format!("request_identities: {e}"))?;
        Ok(ids.len())
    };
    tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| format!("timed out after {timeout:?}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use ssh_agent_mux::MuxAgent;

    /// AF_UNIX socket paths are limited to ~108 bytes (`SUN_LEN`). The
    /// devshell's $TMPDIR can nest deep inside the worktree, so fall back
    /// to /tmp whenever the default tempdir would push a socket path past
    /// the limit. The nix lane's short /build TMPDIR takes the first arm.
    fn short_tempdir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        if dir.path().as_os_str().len() <= 80 {
            dir
        } else {
            tempfile::tempdir_in("/tmp").unwrap()
        }
    }

    #[tokio::test]
    async fn probe_live_mux_with_no_upstreams_counts_zero_keys() {
        let dir = short_tempdir();
        let sock = dir.path().join("mux.sock");

        let mux = tokio::spawn(MuxAgent::run(
            sock.clone(),
            Vec::<PathBuf>::new(),
            None,
            Duration::from_secs(1),
        ));
        for _ in 0..200 {
            if sock.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(sock.exists(), "mux listen socket never appeared");

        let keys = probe_agent(&sock, Duration::from_secs(1)).await.unwrap();
        assert_eq!(keys, 0);
        mux.abort();
    }

    #[tokio::test]
    async fn probe_missing_socket_is_connect_error() {
        let dir = short_tempdir();
        let sock = dir.path().join("absent.sock");

        let err = probe_agent(&sock, Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(err.contains("connect"), "got: {err}");
    }
}
