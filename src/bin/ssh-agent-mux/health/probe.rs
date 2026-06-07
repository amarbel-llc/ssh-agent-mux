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

/// Probe the listen socket and every enabled upstream, all concurrently:
/// the listen probe is joined against a `JoinSet` of upstream probe
/// tasks, so worst-case wall time is ~one `agent_timeout`, not one per
/// dead upstream. Tasks carry their `config.agents` index and results
/// are reassembled into config order; disabled agents stay `None` and
/// spawn no task. `JoinSet::spawn` works on the binary's current_thread
/// runtime (tasks interleave on the one thread, timeouts overlap).
pub async fn probe_all(config: &Config) -> ProbeReport {
    let timeout = Duration::from_secs(config.agent_timeout);
    let listen = probe_agent(&config.listen_path, timeout);
    let upstreams = async {
        let mut probes = tokio::task::JoinSet::new();
        for (index, agent) in config.agents.iter().enumerate() {
            if agent.enabled {
                let path = agent.socket_path.clone();
                probes.spawn(async move { (index, probe_agent(&path, timeout).await) });
            }
        }
        let mut results = vec![None; config.agents.len()];
        while let Some(joined) = probes.join_next().await {
            let (index, result) = joined.expect("upstream probe task panicked");
            results[index] = Some(result);
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

    #[tokio::test(flavor = "current_thread")]
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

    #[tokio::test(flavor = "current_thread")]
    async fn probe_missing_socket_is_connect_error() {
        let dir = short_tempdir();
        let sock = dir.path().join("absent.sock");

        let err = probe_agent(&sock, Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(err.contains("connect"), "got: {err}");
    }

    /// Three enabled upstreams pointing at a socket that accepts
    /// connections but never replies must time out concurrently
    /// (~1 x timeout), not sequentially (3 x timeout). The 2.5s bound
    /// leaves 1.5s of headroom over the ideal ~1s for slow CI while
    /// staying well under the 3s sequential worst case.
    #[tokio::test(flavor = "current_thread")]
    async fn probe_all_times_out_dead_upstreams_concurrently() {
        let dir = short_tempdir();
        let hang = dir.path().join("hang.sock");
        // Bound but never accepted: connect() succeeds via the listen
        // backlog, then request_identities waits forever for a reply.
        let _listener = std::os::unix::net::UnixListener::bind(&hang).unwrap();

        let mut config = crate::cli::Config::default();
        config.listen_path = dir.path().join("absent-listen.sock");
        config.agent_timeout = 1;
        config.agents = (0..3)
            .map(|i| crate::cli::AgentConfig {
                name: format!("hang{i}"),
                socket_path: hang.clone(),
                enabled: true,
            })
            .collect();

        let start = std::time::Instant::now();
        let report = probe_all(&config).await;
        let elapsed = start.elapsed();

        assert_eq!(report.upstreams.len(), 3);
        for upstream in &report.upstreams {
            let err = upstream.as_ref().unwrap().as_ref().unwrap_err();
            assert!(err.contains("timed out"), "got: {err}");
        }
        assert!(
            elapsed < Duration::from_millis(2500),
            "upstream probes should overlap (~1s), took {elapsed:?}"
        );
    }
}
