//! `ssh-agent-mux health`: TAP-emitting service + protocol health checks.
//!
//! Design: docs/plans/2026-06-07-health-subcommand-design.md

use std::io::{self, IsTerminal, Write};

use clap_serde_derive::clap::{self, ValueEnum};
use color_eyre::eyre::Result;
use serde_json::json;
use tap_dancer::{NdjsonWriter, Reporter, TapWriterBuilder};

use crate::cli::Config;

mod service_state;
mod socket_holder;

use service_state::{InstallStatus, ServiceProbe};
use socket_holder::ListenerCheck;

#[derive(ValueEnum, Clone, Copy, PartialEq, Eq)]
pub enum HealthFormat {
    Auto,
    Tap,
    Ndjson,
}

/// Checks always present in the plan, independent of agent count: config
/// valid, service installed, service active, listen socket held by
/// service, listen socket answers. Per-agent "upstream <name> answers"
/// checks are added on top.
const STATIC_CHECKS: usize = 5;

/// Construct the format-dispatching reporter. `Auto` follows tty-ness
/// (TAP text for humans, ndjson for pipes) and keeps the crate's
/// locale/NO_COLOR handling for the human-facing tty case. Forced `Tap`
/// is built with TapWriterBuilder::new — no locale pragma, no
/// locale-formatted numbers, color only on a tty — so piped output
/// stays SGR-free and machine-greppable regardless of LC_ALL/LANG.
fn reporter_for(format: HealthFormat, w: &mut dyn Write, is_tty: bool) -> io::Result<Reporter<'_>> {
    match format {
        HealthFormat::Auto => Reporter::auto(w, is_tty),
        HealthFormat::Tap => {
            let builder = TapWriterBuilder::new(w).color(is_tty);
            Ok(Reporter::Tap(builder.build()?))
        }
        HealthFormat::Ndjson => Ok(Reporter::Ndjson(NdjsonWriter::new(w))),
    }
}

/// Emit every health check as a test point. Later plan tasks replace the
/// remaining `skip(..., "not implemented")` placeholders with real probes;
/// the plan count and descriptions are already final. Host facts arrive
/// pre-resolved in `service` and `listener` so unit tests stay
/// deterministic.
fn emit_checks(
    r: &mut Reporter,
    config_res: &Result<Config>,
    service: &ServiceProbe,
    listener: &ListenerCheck,
) -> io::Result<()> {
    let config = match config_res {
        Err(e) => {
            // Nothing else is checkable without a config: fail the one
            // check we could run and bail out of the whole document.
            r.plan_ahead(1)?;
            r.not_ok_diag("config valid", &[("error", json!(format!("{e:#}")))])?;
            r.bail_out("configuration unusable")?;
            return Ok(());
        }
        Ok(config) => config,
    };

    r.plan_ahead(STATIC_CHECKS + config.agents.len())?;
    r.ok_diag(
        "config valid",
        &[
            ("path", json!(config.config_path.display().to_string())),
            ("agents", json!(config.agents.len())),
        ],
    )?;
    match &service.install {
        InstallStatus::Installed(unit) => r.ok_diag(
            "service installed",
            &[("unit", json!(unit.display().to_string()))],
        )?,
        InstallStatus::NotInstalled => r.skip("service installed", "service not installed")?,
        InstallStatus::Unknown(reason) => r.skip("service installed", reason)?,
    };

    emit_service_active(r, service)?;
    emit_listener_check(r, listener)?;
    r.skip("listen socket answers", "not implemented")?;
    for agent in &config.agents {
        r.skip(
            &format!("upstream {} answers", agent.name),
            "not implemented",
        )?;
    }
    Ok(())
}

/// Emit the "service active" point. The running MainPID consumed by the
/// listener-identity check travels separately via
/// [`ServiceProbe::active_main_pid`], resolved host-side in `run` so
/// `emit_checks` stays host-independent.
fn emit_service_active(r: &mut Reporter, service: &ServiceProbe) -> io::Result<()> {
    if !matches!(service.install, InstallStatus::Installed(_)) {
        r.skip("service active", "service not installed")?;
        return Ok(());
    }
    let Some(state) = &service.state else {
        r.skip("service active", service_state::MANAGER_UNAVAILABLE)?;
        return Ok(());
    };
    if state.active_state.as_deref() == Some("active") {
        // MainPID can legitimately be absent (systemd reports MainPID=0
        // for some unit shapes); emit the diagnostic only when known.
        match state.main_pid {
            Some(pid) => r.ok_diag("service active", &[("main-pid", json!(pid))])?,
            None => r.ok("service active")?,
        };
    } else {
        r.not_ok_diag(
            "service active",
            &[(
                "active-state",
                json!(state.active_state.as_deref().unwrap_or("unknown")),
            )],
        )?;
    }
    Ok(())
}

/// Render the "listen socket held by service" point from the pre-resolved
/// [`ListenerCheck`] verdict (see `socket_holder` for how it is gathered).
fn emit_listener_check(r: &mut Reporter, check: &ListenerCheck) -> io::Result<()> {
    const DESC: &str = "listen socket held by service";
    match check {
        ListenerCheck::Skipped(reason) => r.skip(DESC, reason),
        #[cfg(any(target_os = "linux", test))]
        ListenerCheck::NotFound => r.not_ok_diag(
            DESC,
            &[("error", json!("listen path not present in /proc/net/unix"))],
        ),
        #[cfg(any(target_os = "linux", test))]
        ListenerCheck::HeldByService { main_pid } => {
            r.ok_diag(DESC, &[("main-pid", json!(main_pid))])
        }
        #[cfg(any(target_os = "linux", test))]
        ListenerCheck::HeldByOther {
            holder_pid,
            holder_cgroup,
        } => {
            // Best-effort holder facts: omit pairs that could not be
            // resolved rather than emitting nulls.
            let mut diags: Vec<(&str, serde_json::Value)> = Vec::new();
            if let Some(pid) = holder_pid {
                diags.push(("holder-pid", json!(pid)));
            }
            if let Some(cgroup) = holder_cgroup {
                diags.push(("holder-cgroup", json!(cgroup)));
            }
            r.not_ok_diag(DESC, &diags)
        }
    }?;
    Ok(())
}

pub async fn run(config_res: Result<Config>, format: HealthFormat) -> Result<()> {
    let stdout = io::stdout();
    let is_tty = stdout.is_terminal();
    let mut out = stdout.lock();

    let service = service_state::probe().await;
    let listener = socket_holder::probe(
        config_res
            .as_ref()
            .ok()
            .map(|config| config.listen_path.as_path()),
        service.active_main_pid(),
    );
    let mut reporter = reporter_for(format, &mut out, is_tty)?;
    emit_checks(&mut reporter, &config_res, &service, &listener)?;
    reporter.finish()?;
    let has_failures = reporter.has_failures();
    drop(reporter);
    out.flush()?;

    if has_failures {
        // Deterministic exit code for scripts; stdout already flushed.
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap_serde_derive::ClapSerde;

    fn config_with_one_agent() -> Config {
        let parsed = toml::from_str::<<Config as ClapSerde>::Opt>(
            r#"
[[agents]]
name = "fake"
socket-path = "/tmp/does-not-exist.sock"
"#,
        )
        .unwrap();
        let mut config = Config::from(parsed);
        config.config_path = "/tmp/test-config.toml".into();
        config
    }

    /// Deterministic host-independent default: service not installed →
    /// both service checks skip, matching the sandboxed bats lane.
    fn probe_not_installed() -> ServiceProbe {
        ServiceProbe {
            install: InstallStatus::NotInstalled,
            state: None,
        }
    }

    fn probe_installed(state: Option<service_state::ServiceState>) -> ServiceProbe {
        ServiceProbe {
            install: InstallStatus::Installed("/home/u/.config/systemd/user/mux.service".into()),
            state,
        }
    }

    /// Deterministic listener default matching `probe_not_installed`: an
    /// inactive service yields nothing to compare the holder against.
    fn listener_not_active() -> ListenerCheck {
        ListenerCheck::Skipped("service not active".to_string())
    }

    fn emit_full(
        format: HealthFormat,
        is_tty: bool,
        config_res: &Result<Config>,
        service: &ServiceProbe,
        listener: &ListenerCheck,
    ) -> (String, bool) {
        let mut buf = Vec::new();
        let mut reporter = reporter_for(format, &mut buf, is_tty).unwrap();
        emit_checks(&mut reporter, config_res, service, listener).unwrap();
        reporter.finish().unwrap();
        let has_failures = reporter.has_failures();
        drop(reporter);
        (String::from_utf8(buf).unwrap(), has_failures)
    }

    fn emit_with_probe(
        format: HealthFormat,
        is_tty: bool,
        config_res: &Result<Config>,
        service: &ServiceProbe,
    ) -> (String, bool) {
        emit_full(format, is_tty, config_res, service, &listener_not_active())
    }

    fn emit(format: HealthFormat, is_tty: bool, config_res: &Result<Config>) -> (String, bool) {
        emit_with_probe(format, is_tty, config_res, &probe_not_installed())
    }

    /// Tap-format emit with an installed+active service and the given
    /// listener verdict — the fixture for the listener-check tests.
    fn emit_listener(listener: &ListenerCheck) -> (String, bool) {
        let config_res = Ok(config_with_one_agent());
        let probe = probe_installed(Some(service_state::ServiceState {
            active_state: Some("active".into()),
            main_pid: Some(16891),
        }));
        emit_full(HealthFormat::Tap, false, &config_res, &probe, listener)
    }

    #[test]
    fn tap_valid_config_emits_full_plan() {
        let config_res = Ok(config_with_one_agent());
        let (out, has_failures) = emit(HealthFormat::Tap, false, &config_res);
        assert!(!has_failures);
        // Forced tap uses TapWriterBuilder::new (no locale): the plan must
        // directly follow the version line with no `pragma` line between,
        // regardless of LC_ALL/LC_NUMERIC/LANG in the test environment.
        assert!(out.starts_with("TAP version 14\n1..6\n"), "got: {out}");
        assert!(out.contains("ok 1 - config valid"), "got: {out}");
        assert!(
            out.contains("path: \"/tmp/test-config.toml\""),
            "got: {out}"
        );
        assert!(out.contains("agents: 1"), "got: {out}");
        assert!(
            out.contains("ok 6 - upstream fake answers # SKIP not implemented"),
            "got: {out}"
        );
    }

    #[test]
    fn tap_bad_config_bails_out() {
        let config_res = Err(color_eyre::eyre::eyre!("unknown field `not-a-real-key`"));
        let (out, has_failures) = emit(HealthFormat::Tap, false, &config_res);
        assert!(has_failures);
        assert!(out.contains("1..1"), "got: {out}");
        assert!(out.contains("not ok 1 - config valid"), "got: {out}");
        assert!(out.contains("not-a-real-key"), "got: {out}");
        assert!(
            out.contains("Bail out! configuration unusable"),
            "got: {out}"
        );
    }

    #[test]
    fn ndjson_valid_config_emits_records_and_summary() {
        let config_res = Ok(config_with_one_agent());
        let (out, has_failures) = emit(HealthFormat::Ndjson, false, &config_res);
        assert!(!has_failures);
        let lines: Vec<&str> = out.lines().collect();
        for line in &lines {
            serde_json::from_str::<serde_json::Value>(line).expect("every line parses as JSON");
        }
        assert_eq!(lines.first().unwrap(), &"{\"type\":\"plan\",\"count\":6}");
        // Integer diagnostics stay integers in ndjson.
        assert!(out.contains("\"agents\":1"), "got: {out}");
        let summary: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
        assert_eq!(summary["type"], "summary");
        assert_eq!(summary["passed"], 1);
        assert_eq!(summary["skipped"], 5);
        assert_eq!(summary["failed"], 0);
        // plan + (STATIC_CHECKS + 1 agent) tests + summary
        assert_eq!(lines.len(), 1 + (STATIC_CHECKS + 1) + 1);
    }

    #[test]
    fn ndjson_bad_config_bails_out() {
        let config_res = Err(color_eyre::eyre::eyre!("unknown field `not-a-real-key`"));
        let (out, has_failures) = emit(HealthFormat::Ndjson, false, &config_res);
        assert!(has_failures);
        assert!(out.contains("\"type\":\"bailout\""), "got: {out}");
        let summary: serde_json::Value = serde_json::from_str(out.lines().last().unwrap()).unwrap();
        assert_eq!(summary["type"], "summary");
        assert_eq!(summary["failed"], 1);
        assert_eq!(summary["bailed"], true);
    }

    #[test]
    fn auto_non_tty_is_ndjson() {
        let config_res = Ok(config_with_one_agent());
        let (out, _) = emit(HealthFormat::Auto, false, &config_res);
        assert!(out.starts_with('{'), "got: {out}");
    }

    #[test]
    fn auto_tty_is_tap() {
        let config_res = Ok(config_with_one_agent());
        let (out, _) = emit(HealthFormat::Auto, true, &config_res);
        assert!(out.starts_with("TAP version 14\n"), "got: {out}");
    }

    #[test]
    fn service_not_installed_skips_both_service_checks() {
        let config_res = Ok(config_with_one_agent());
        let (out, has_failures) = emit(HealthFormat::Tap, false, &config_res);
        assert!(!has_failures);
        assert!(
            out.contains("ok 2 - service installed # SKIP service not installed"),
            "got: {out}"
        );
        assert!(
            out.contains("ok 3 - service active # SKIP service not installed"),
            "got: {out}"
        );
        assert!(
            out.contains("ok 4 - listen socket held by service # SKIP service not active"),
            "got: {out}"
        );
    }

    #[test]
    fn listener_held_by_service_is_ok() {
        let (out, has_failures) = emit_listener(&ListenerCheck::HeldByService { main_pid: 16891 });
        assert!(!has_failures);
        assert!(
            out.contains("ok 4 - listen socket held by service"),
            "got: {out}"
        );
    }

    #[test]
    fn listener_path_not_bound_fails() {
        let (out, has_failures) = emit_listener(&ListenerCheck::NotFound);
        assert!(has_failures);
        assert!(
            out.contains("not ok 4 - listen socket held by service"),
            "got: {out}"
        );
        assert!(
            out.contains("listen path not present in /proc/net/unix"),
            "got: {out}"
        );
    }

    #[test]
    fn listener_held_by_foreign_process_fails_with_holder_facts() {
        let (out, has_failures) = emit_listener(&ListenerCheck::HeldByOther {
            holder_pid: Some(4242),
            holder_cgroup: Some("0::/user.slice/foreign.service".to_string()),
        });
        assert!(has_failures);
        assert!(
            out.contains("not ok 4 - listen socket held by service"),
            "got: {out}"
        );
        assert!(out.contains("holder-pid: 4242"), "got: {out}");
        assert!(
            out.contains("holder-cgroup: \"0::/user.slice/foreign.service\""),
            "got: {out}"
        );
    }

    #[test]
    fn listener_unresolved_holder_omits_diags() {
        let (out, has_failures) = emit_listener(&ListenerCheck::HeldByOther {
            holder_pid: None,
            holder_cgroup: None,
        });
        assert!(has_failures);
        assert!(
            out.contains("not ok 4 - listen socket held by service"),
            "got: {out}"
        );
        assert!(!out.contains("holder-pid"), "got: {out}");
        assert!(!out.contains("holder-cgroup"), "got: {out}");
    }

    #[test]
    fn listener_proc_unreadable_skips_with_reason() {
        let (out, has_failures) = emit_listener(&ListenerCheck::Skipped(
            "cannot read /proc/net/unix: permission denied".to_string(),
        ));
        assert!(!has_failures);
        assert!(
            out.contains("ok 4 - listen socket held by service # SKIP cannot read /proc/net/unix"),
            "got: {out}"
        );
    }

    #[test]
    fn service_installed_and_active_emits_unit_and_pid() {
        let config_res = Ok(config_with_one_agent());
        let probe = probe_installed(Some(service_state::ServiceState {
            active_state: Some("active".into()),
            main_pid: Some(16891),
        }));
        let (out, has_failures) = emit_with_probe(HealthFormat::Tap, false, &config_res, &probe);
        assert!(!has_failures);
        assert!(out.contains("ok 2 - service installed"), "got: {out}");
        assert!(
            out.contains("unit: \"/home/u/.config/systemd/user/mux.service\""),
            "got: {out}"
        );
        assert!(out.contains("ok 3 - service active"), "got: {out}");
        assert!(out.contains("main-pid: 16891"), "got: {out}");
    }

    #[test]
    fn service_active_without_pid_is_still_ok() {
        let config_res = Ok(config_with_one_agent());
        let probe = probe_installed(Some(service_state::ServiceState {
            active_state: Some("active".into()),
            main_pid: None,
        }));
        let (out, has_failures) = emit_with_probe(HealthFormat::Tap, false, &config_res, &probe);
        assert!(!has_failures);
        assert!(out.contains("ok 3 - service active"), "got: {out}");
        assert!(!out.contains("main-pid"), "got: {out}");
    }

    #[test]
    fn service_manager_unavailable_skips_active_check() {
        let config_res = Ok(config_with_one_agent());
        let probe = probe_installed(None);
        let (out, has_failures) = emit_with_probe(HealthFormat::Tap, false, &config_res, &probe);
        assert!(!has_failures);
        assert!(out.contains("ok 2 - service installed"), "got: {out}");
        assert!(
            out.contains(&format!(
                "ok 3 - service active # SKIP {}",
                service_state::MANAGER_UNAVAILABLE
            )),
            "got: {out}"
        );
    }

    #[test]
    fn service_inactive_fails_active_check() {
        let config_res = Ok(config_with_one_agent());
        let probe = probe_installed(Some(service_state::ServiceState {
            active_state: Some("failed".into()),
            main_pid: None,
        }));
        let (out, has_failures) = emit_with_probe(HealthFormat::Tap, false, &config_res, &probe);
        assert!(has_failures);
        assert!(out.contains("not ok 3 - service active"), "got: {out}");
        assert!(out.contains("active-state: \"failed\""), "got: {out}");
    }
}
