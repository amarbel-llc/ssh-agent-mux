//! Service-manager state probing for the `health` subcommand.
//!
//! Resolved once on the real host by [`probe`] and threaded into
//! `emit_checks` as plain data, so the buffer-backed unit tests in
//! `health.rs` never depend on host service state.

use std::path::PathBuf;

/// Skip reason emitted when the service manager cannot be queried
/// (sandbox/CI without a user service manager).
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

pub(crate) fn probe() -> ServiceProbe {
    let install = install_status();
    let state = match install {
        InstallStatus::Installed(_) => query_service_state_host(),
        _ => None,
    };
    ServiceProbe { install, state }
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
fn query_service_state_host() -> Option<ServiceState> {
    query_service_state(crate::service::SYSTEMD_UNIT_FILENAME)
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

/// `None` ⇒ systemctl unavailable (sandbox/CI) → caller skips the check.
#[cfg(target_os = "linux")]
pub(crate) fn query_service_state(unit: &str) -> Option<ServiceState> {
    let out = std::process::Command::new("systemctl")
        .args(["--user", "show", "-p", "ActiveState,MainPID", unit])
        .output()
        .ok()?;
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

/// Exit-status-only probe: `launchctl list <label>` exits 0 iff the job is
/// loaded in the current user's domain. No MainPID is extracted.
#[cfg(target_os = "macos")]
fn query_service_state_host() -> Option<ServiceState> {
    let out = std::process::Command::new("launchctl")
        .args(["list", crate::service::SERVICE_LABEL])
        .output()
        .ok()?;
    let active_state = if out.status.success() {
        "active"
    } else {
        "inactive"
    };
    Some(ServiceState {
        active_state: Some(active_state.to_string()),
        main_pid: None,
    })
}

// ---------------------------------------------------------------------------
// Unsupported platforms
// ---------------------------------------------------------------------------

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unit_dest() -> color_eyre::eyre::Result<PathBuf> {
    color_eyre::eyre::bail!("service management unsupported on this platform")
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn query_service_state_host() -> Option<ServiceState> {
    None
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
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
