//! Listener-identity probing for the `health` subcommand.
//!
//! Verifies the configured listen socket is actually held by the service
//! process, catching the failure mode where a foreign process is bound at
//! the configured path (or a stale socket file points at nothing). Linux
//! only: relies on `/proc/net/unix` and `/proc/<pid>/fd`. On macOS the
//! launchd prober surfaces a main pid, but mapping a bound socket path to
//! its holding process has no /proc equivalent — a future variant could
//! shell out to `lsof -U`.
//!
//! Split for testability (same shape as `service_state`): the pure parser
//! [`unix_socket_inode`] and the [`ListenerCheck`] verdict enum are
//! unit-tested; the /proc readers are thin host-only glue that merely
//! assemble a verdict, which `health.rs` renders (and unit-tests) from
//! fabricated values.

use std::path::Path;

use super::service_state::ServiceProbe;

/// Skip reason when the service is not running at all (not installed,
/// manager unavailable, or inactive) — there is no holder to compare.
pub(crate) const SKIP_NOT_ACTIVE: &str = "service not active";

/// Skip reason when the service *is* active but the manager reported no
/// main pid (`MainPID=0` unit shapes: oneshot, notify without a pid) —
/// distinct from [`SKIP_NOT_ACTIVE`] so the skip cannot contradict an
/// "ok - service active" point earlier in the same document.
pub(crate) const SKIP_NO_MAIN_PID: &str = "service main pid unavailable";

/// Skip reason on platforms without /proc (see module docs: an lsof-based
/// variant could lift this).
#[cfg(not(target_os = "linux"))]
const NOT_IMPLEMENTED: &str = "not implemented on macos";

/// Verdict for the "listen socket held by service" point. Resolved once on
/// the live host by [`probe`] and threaded into `emit_checks` as plain
/// data, so the buffer-backed unit tests in `health.rs` never read /proc.
pub(crate) enum ListenerCheck {
    /// The check could not run (service inactive, /proc unreadable, or a
    /// platform without /proc); emitted as a TAP skip with this reason.
    Skipped(String),
    /// No listening socket bound at the configured path at all.
    #[cfg(any(target_os = "linux", test))]
    NotFound,
    /// The service's MainPID holds the listening socket — healthy.
    #[cfg(any(target_os = "linux", test))]
    HeldByService { main_pid: u32 },
    /// Bound, but by some other process — the foreign-holder failure mode.
    /// Holder facts are best-effort: `holder` is `None` when the pid could
    /// not be resolved; the cgroup (only resolvable with a pid in hand) may
    /// still be `None` inside. Unresolved facts are omitted from diags.
    #[cfg(any(target_os = "linux", test))]
    HeldByOther {
        holder: Option<(u32, Option<String>)>,
    },
}

/// Resolve the listener-identity facts on the live host, consuming the
/// service-manager facts from `service` (see
/// [`ServiceProbe::active_main_pid`]). `listen_path` is `None` only when
/// the config failed to load, in which case `emit_checks` bails out before
/// this verdict is ever emitted.
pub(crate) fn probe(listen_path: Option<&Path>, service: &ServiceProbe) -> ListenerCheck {
    let Some(listen_path) = listen_path else {
        // Dead in practice (a config error bails the document before this
        // verdict is rendered); honest reason in case that ever changes.
        return ListenerCheck::Skipped("config not loaded".to_string());
    };
    if !service.is_active() {
        return ListenerCheck::Skipped(SKIP_NOT_ACTIVE.to_string());
    }
    let Some(main_pid) = service.active_main_pid() else {
        return ListenerCheck::Skipped(SKIP_NO_MAIN_PID.to_string());
    };
    probe_platform(listen_path, main_pid)
}

#[cfg(target_os = "linux")]
fn probe_platform(listen_path: &Path, main_pid: u32) -> ListenerCheck {
    let proc_net_unix = match std::fs::read_to_string("/proc/net/unix") {
        Ok(content) => content,
        // Can't inspect ⇒ honest skip (check not performed), not a
        // failure: an unreadable /proc says nothing about the service.
        Err(e) => return ListenerCheck::Skipped(format!("cannot read /proc/net/unix: {e}")),
    };
    let Some(inode) = unix_socket_inode(&proc_net_unix, listen_path) else {
        return ListenerCheck::NotFound;
    };
    if pid_holds_socket_inode(main_pid, inode) {
        return ListenerCheck::HeldByService { main_pid };
    }
    let holder = find_socket_holder(inode).map(|pid| (pid, pid_cgroup(pid)));
    ListenerCheck::HeldByOther { holder }
}

#[cfg(not(target_os = "linux"))]
fn probe_platform(_listen_path: &Path, _main_pid: u32) -> ListenerCheck {
    // No /proc here: even with launchd's main pid in hand, mapping the
    // bound socket path to its holder needs an lsof-based variant.
    ListenerCheck::Skipped(NOT_IMPLEMENTED.to_string())
}

/// Parse /proc/net/unix content; return the inode of the listening socket
/// bound at `path`. Column layout: Num RefCount Protocol Flags Type St
/// Inode Path (Path may be absent). Filters on the `__SO_ACCEPTCON` flag:
/// connections accepted by the listener appear under the same path, and
/// the holder of the *listening* socket is what identifies the server.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn unix_socket_inode(proc_net_unix: &str, path: &Path) -> Option<u64> {
    const SO_ACCEPTCON: u32 = 0x0001_0000;
    let want = path.to_str()?;
    proc_net_unix.lines().skip(1).find_map(|line| {
        let mut cols = line.split_whitespace();
        let flags = u32::from_str_radix(cols.by_ref().nth(3)?, 16).ok()?;
        if flags & SO_ACCEPTCON == 0 {
            return None;
        }
        let inode = cols.by_ref().nth(2)?.parse::<u64>().ok()?;
        (cols.next()? == want).then_some(inode)
    })
}

/// True if /proc/<pid>/fd contains a link to socket:[inode].
#[cfg(target_os = "linux")]
fn pid_holds_socket_inode(pid: u32, inode: u64) -> bool {
    let needle = format!("socket:[{inode}]");
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .map(|entries| {
            entries.flatten().any(|e| {
                std::fs::read_link(e.path())
                    .map(|t| t.to_string_lossy() == needle)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Best-effort: scan /proc for the pid holding the socket (foreign-holder
/// diagnostic). Permission errors are skipped silently.
#[cfg(target_os = "linux")]
fn find_socket_holder(inode: u64) -> Option<u32> {
    std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .find(|pid| pid_holds_socket_inode(*pid, inode))
}

/// First line of /proc/<pid>/cgroup (on cgroup v2, the pid's full unit
/// path) — names the foreign holder in diagnostics.
#[cfg(target_os = "linux")]
fn pid_cgroup(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
        .ok()
        .map(|s| s.lines().next().unwrap_or("").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROC_NET_UNIX: &str = "\
Num       RefCount Protocol Flags    Type St Inode Path
0000000000000000: 00000002 00000000 00010000 0001 01 72114 /home/sasha/.local/state/ssh/mux-agent.sock
0000000000000000: 00000002 00000000 00010000 0001 01 28168 /home/sasha/.local/state/ssh/pivy-agent.sock
";

    #[test]
    fn finds_inode_for_exact_path() {
        assert_eq!(
            unix_socket_inode(
                PROC_NET_UNIX,
                std::path::Path::new("/home/sasha/.local/state/ssh/mux-agent.sock")
            ),
            Some(72114)
        );
    }

    #[test]
    fn missing_path_yields_none() {
        assert_eq!(
            unix_socket_inode(PROC_NET_UNIX, std::path::Path::new("/nope.sock")),
            None
        );
    }

    /// Accepted connections show up in /proc/net/unix under the listener's
    /// path (St=03, no __SO_ACCEPTCON flag); only the listening socket's
    /// inode identifies the server.
    #[test]
    fn skips_accepted_connections_sharing_the_path() {
        let content = "\
Num       RefCount Protocol Flags    Type St Inode Path
0000000000000000: 00000003 00000000 00000000 0001 03 99001 /home/sasha/.local/state/ssh/mux-agent.sock
0000000000000000: 00000002 00000000 00010000 0001 01 72114 /home/sasha/.local/state/ssh/mux-agent.sock
";
        assert_eq!(
            unix_socket_inode(
                content,
                std::path::Path::new("/home/sasha/.local/state/ssh/mux-agent.sock")
            ),
            Some(72114)
        );
    }

    use super::super::service_state::{InstallStatus, ServiceState};

    fn skip_reason(check: ListenerCheck) -> String {
        let ListenerCheck::Skipped(reason) = check else {
            panic!("expected Skipped");
        };
        reason
    }

    /// Pure paths through `probe`: a service that is not running (here:
    /// not even installed) skips without touching /proc.
    #[test]
    fn probe_inactive_service_skips_as_not_active() {
        let service = ServiceProbe {
            install: InstallStatus::NotInstalled,
            state: None,
        };
        let check = probe(Some(std::path::Path::new("/tmp/x.sock")), &service);
        assert_eq!(skip_reason(check), "service not active");
    }

    /// Active-but-MainPID=0 unit shapes (oneshot, notify without a pid)
    /// must not skip as "service not active" — that would contradict the
    /// "ok - service active" point emitted just before.
    #[test]
    fn probe_active_without_main_pid_skips_with_distinct_reason() {
        let service = ServiceProbe {
            install: InstallStatus::Installed("/home/u/.config/systemd/user/mux.service".into()),
            state: Some(ServiceState {
                active_state: Some("active".to_string()),
                main_pid: None,
            }),
        };
        let check = probe(Some(std::path::Path::new("/tmp/x.sock")), &service);
        assert_eq!(skip_reason(check), "service main pid unavailable");
    }
}
