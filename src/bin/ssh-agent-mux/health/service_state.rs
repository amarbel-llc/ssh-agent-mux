//! Service-manager state probing for the `health` subcommand.
//!
//! Resolved once on the real host by [`probe`] (async so the
//! service-manager subprocess can be time-bounded) and threaded into
//! `emit_checks` as plain data, so the buffer-backed unit tests in
//! `health.rs` never depend on host service state.

use std::path::PathBuf;

/// Skip reason emitted when the service manager cannot be queried
/// (sandbox/CI without a user service manager, or a query that exceeded
/// [`MANAGER_TIMEOUT`]).
#[cfg(target_os = "linux")]
pub(crate) const MANAGER_UNAVAILABLE: &str = "systemctl unavailable";
#[cfg(target_os = "macos")]
pub(crate) const MANAGER_UNAVAILABLE: &str = "launchctl unavailable";
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) const MANAGER_UNAVAILABLE: &str = "service manager unavailable";

/// Whether the unit file (plist on macOS) is installed.
pub(crate) enum InstallStatus {
    /// Unit file present at this path.
    Installed(PathBuf),
    /// Unit file absent from the install destination.
    NotInstalled,
    /// Destination could not even be determined (e.g. HOME unset, or an
    /// unsupported platform); carries the skip reason.
    Unknown(String),
}

pub(crate) struct ServiceState {
    pub(crate) active_state: Option<String>,
    /// Service main PID; `MainPID=0` (no main process) normalizes to `None`.
    pub(crate) main_pid: Option<u32>,
}

/// Host service-manager facts consumed by `emit_checks`.
pub(crate) struct ServiceProbe {
    pub(crate) install: InstallStatus,
    /// `None` ⇒ the service manager was unavailable. Only queried when the
    /// unit is installed; not-installed units skip the active check anyway.
    pub(crate) state: Option<ServiceState>,
}

impl ServiceProbe {
    /// True when the unit is installed, the manager answered, and
    /// ActiveState=active — the same conditions under which
    /// `emit_service_active` reports "ok". Lets the listener-identity
    /// probe (`socket_holder::probe`) tell "service not running" apart
    /// from "running but no MainPID" when [`Self::active_main_pid`] is
    /// `None`.
    pub(crate) fn is_active(&self) -> bool {
        matches!(self.install, InstallStatus::Installed(_))
            && self
                .state
                .as_ref()
                .is_some_and(|state| state.active_state.as_deref() == Some("active"))
    }

    /// MainPID of the running service: `Some` only when [`Self::is_active`]
    /// and the manager reported a nonzero MainPID. Consumed by the
    /// listener-identity probe (`socket_holder::probe`).
    pub(crate) fn active_main_pid(&self) -> Option<u32> {
        if !self.is_active() {
            return None;
        }
        self.state.as_ref()?.main_pid
    }
}

pub(crate) async fn probe() -> ServiceProbe {
    let install = install_status();
    let state = match install {
        InstallStatus::Installed(_) => query_service_state_host().await,
        _ => None,
    };
    ServiceProbe { install, state }
}

/// Hard deadline on service-manager subprocesses. `systemctl --user show`
/// can hang indefinitely in exactly the environments a health tool
/// diagnoses (stale DBUS_SESSION_BUS_ADDRESS, broken systemd stub in a
/// container, crashed user session), and a hang would violate the
/// `None ⇒ unavailable` contract of the query fns.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const MANAGER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Run the service manager with [`MANAGER_TIMEOUT`] as a hard deadline.
/// `None` on spawn failure or timeout — both fold into the existing
/// "manager unavailable" skip path. `kill_on_drop` ensures a hung
/// systemctl/launchctl is killed when the timeout drops the future, so it
/// cannot outlive the health run.
#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn manager_output(cmd: &str, args: &[&str]) -> Option<std::process::Output> {
    let output = tokio::process::Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    tokio::time::timeout(MANAGER_TIMEOUT, output)
        .await
        .ok()?
        .ok()
}

fn install_status() -> InstallStatus {
    match unit_dest() {
        Ok(path) if path.exists() => InstallStatus::Installed(path),
        Ok(_) => InstallStatus::NotInstalled,
        Err(e) => InstallStatus::Unknown(format!("{e:#}")),
    }
}

// ---------------------------------------------------------------------------
// Linux (systemd)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn unit_dest() -> color_eyre::eyre::Result<PathBuf> {
    crate::service::systemd_unit_dest()
}

#[cfg(target_os = "linux")]
async fn query_service_state_host() -> Option<ServiceState> {
    query_service_state(crate::service::SYSTEMD_UNIT_FILENAME).await
}

#[cfg(target_os = "linux")]
pub(crate) fn parse_systemctl_show(out: &str) -> ServiceState {
    let mut active_state = None;
    let mut main_pid = None;
    for line in out.lines() {
        if let Some(v) = line.strip_prefix("ActiveState=") {
            active_state = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("MainPID=") {
            main_pid = v.trim().parse::<u32>().ok().filter(|p| *p != 0);
        }
    }
    ServiceState {
        active_state,
        main_pid,
    }
}

/// `None` ⇒ systemctl unavailable (sandbox/CI) or unresponsive (timed
/// out) → caller skips the check.
#[cfg(target_os = "linux")]
pub(crate) async fn query_service_state(unit: &str) -> Option<ServiceState> {
    let out = manager_output(
        "systemctl",
        &["--user", "show", "-p", "ActiveState,MainPID", unit],
    )
    .await?;
    if !out.status.success() {
        return None;
    }
    Some(parse_systemctl_show(&String::from_utf8_lossy(&out.stdout)))
}

// ---------------------------------------------------------------------------
// macOS (launchd)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn unit_dest() -> color_eyre::eyre::Result<PathBuf> {
    crate::service::plist_dest()
}

/// `launchctl list <label>` exits 0 iff the job is *loaded* in the current
/// user's domain — loaded ≠ running (a loaded-but-stopped job still exits
/// 0). To distinguish, parse stdout: launchctl prints the job's property
/// dict, which contains a `"PID" = <n>;` entry only while the job has a
/// running process. We use `list <label>` rather than `print
/// gui/$UID/<label>` because `list` requires no UID lookup and its
/// exit-status/stdout contract is sufficient for this narrow probe.
#[cfg(target_os = "macos")]
async fn query_service_state_host() -> Option<ServiceState> {
    let out = manager_output("launchctl", &["list", crate::service::SERVICE_LABEL]).await?;
    if !out.status.success() {
        // Non-zero exit ⇒ label not loaded at all.
        return Some(ServiceState {
            active_state: Some("inactive".to_string()),
            main_pid: None,
        });
    }
    Some(parse_launchctl_list(&String::from_utf8_lossy(&out.stdout)))
}

/// Parse `launchctl list <label>` stdout (the job's property dict). A
/// `"PID" = <n>;` entry is present iff the job has a running process →
/// active with that pid; absent ⇒ the job is loaded but not running, which
/// the active check must report honestly rather than as "active".
///
/// Pure fn on `&str` so it stays unit-testable on every platform; compiled
/// under `test` everywhere, but only *called* from the macOS prober.
#[cfg(any(target_os = "macos", test))]
fn parse_launchctl_list(out: &str) -> ServiceState {
    for line in out.lines() {
        let Some(rest) = line.trim_start().strip_prefix("\"PID\"") else {
            continue;
        };
        let Some(value) = rest.split('=').nth(1) else {
            continue;
        };
        if let Ok(pid) = value.trim().trim_end_matches(';').trim_end().parse::<u32>() {
            return ServiceState {
                active_state: Some("active".to_string()),
                main_pid: Some(pid),
            };
        }
    }
    ServiceState {
        active_state: Some("loaded-not-running".to_string()),
        main_pid: None,
    }
}

// ---------------------------------------------------------------------------
// Unsupported platforms
// ---------------------------------------------------------------------------

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unit_dest() -> color_eyre::eyre::Result<PathBuf> {
    color_eyre::eyre::bail!("service management unsupported on this platform")
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
async fn query_service_state_host() -> Option<ServiceState> {
    None
}

// Platform-neutral tests: parse_launchctl_list is a pure fn on &str and is
// compiled under `test` on every platform, so these run on Linux hosts too.
#[cfg(test)]
mod launchctl_tests {
    use super::*;

    #[test]
    fn running_job_yields_active_with_pid() {
        let out = "{\n\t\"LimitLoadToSessionType\" = \"Aqua\";\n\t\"Label\" = \"net.ross-williams.ssh-agent-mux\";\n\t\"OnDemand\" = false;\n\t\"LastExitStatus\" = 0;\n\t\"PID\" = 16891;\n\t\"Program\" = \"/usr/local/bin/ssh-agent-mux\";\n};\n";
        let st = parse_launchctl_list(out);
        assert_eq!(st.active_state.as_deref(), Some("active"));
        assert_eq!(st.main_pid, Some(16891));
    }

    #[test]
    fn stopped_job_without_pid_is_loaded_not_running() {
        let out = "{\n\t\"Label\" = \"net.ross-williams.ssh-agent-mux\";\n\t\"OnDemand\" = false;\n\t\"LastExitStatus\" = 1;\n};\n";
        let st = parse_launchctl_list(out);
        assert_eq!(st.active_state.as_deref(), Some("loaded-not-running"));
        assert_eq!(st.main_pid, None);
    }
}

// Gated to Linux only because parse_systemctl_show itself is Linux-only
// cfg'd code; don't copy this gate for platform-neutral parsing code (see
// launchctl_tests above).
#[cfg(all(test, target_os = "linux"))]
mod systemd_tests {
    use super::*;

    #[test]
    fn parses_active_state_and_main_pid() {
        let out = "ActiveState=active\nMainPID=16891\n";
        let st = parse_systemctl_show(out);
        assert_eq!(st.active_state.as_deref(), Some("active"));
        assert_eq!(st.main_pid, Some(16891));
    }

    #[test]
    fn failed_unit_has_zero_main_pid() {
        let out = "ActiveState=failed\nMainPID=0\n";
        let st = parse_systemctl_show(out);
        assert_eq!(st.active_state.as_deref(), Some("failed"));
        assert_eq!(st.main_pid, None); // 0 normalized to None
    }
}
