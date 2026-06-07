//! `ssh-agent-mux health`: TAP-emitting service + protocol health checks.
//!
//! Design: docs/plans/2026-06-07-health-subcommand-design.md

use std::io::{self, IsTerminal, Write};

use clap_serde_derive::clap::{self, ValueEnum};
use color_eyre::eyre::Result;
use serde_json::json;
use tap_dancer::{NdjsonWriter, Reporter, TapWriterBuilder};

use crate::cli::Config;

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
/// (TAP text for humans, ndjson for pipes); `Tap`/`Ndjson` force the
/// format. TapWriterBuilder::auto derives color from NO_COLOR only (the
/// crate never sniffs fds), so forced-TAP output bound for a pipe gets
/// color switched off here to stay SGR-free and machine-greppable.
fn reporter_for(format: HealthFormat, w: &mut dyn Write, is_tty: bool) -> io::Result<Reporter<'_>> {
    match format {
        HealthFormat::Auto => Reporter::auto(w, is_tty),
        HealthFormat::Tap => {
            let mut builder = TapWriterBuilder::auto(w);
            if !is_tty {
                builder = builder.color(false);
            }
            Ok(Reporter::Tap(builder.build()?))
        }
        HealthFormat::Ndjson => Ok(Reporter::Ndjson(NdjsonWriter::new(w))),
    }
}

/// Emit every health check as a test point. Later plan tasks replace the
/// `skip(..., "not implemented")` placeholders with real probes; the
/// plan count and descriptions are already final.
fn emit_checks(r: &mut Reporter, config_res: &Result<Config>) -> io::Result<()> {
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
    r.skip("service installed", "not implemented")?;
    r.skip("service active", "not implemented")?;
    r.skip("listen socket held by service", "not implemented")?;
    r.skip("listen socket answers", "not implemented")?;
    for agent in &config.agents {
        r.skip(&format!("upstream {} answers", agent.name), "not implemented")?;
    }
    Ok(())
}

pub async fn run(config_res: Result<Config>, format: HealthFormat) -> Result<()> {
    let stdout = io::stdout();
    let is_tty = stdout.is_terminal();
    let mut out = stdout.lock();

    let mut reporter = reporter_for(format, &mut out, is_tty)?;
    emit_checks(&mut reporter, &config_res)?;
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

    fn emit(format: HealthFormat, is_tty: bool, config_res: &Result<Config>) -> (String, bool) {
        let mut buf = Vec::new();
        let mut reporter = reporter_for(format, &mut buf, is_tty).unwrap();
        emit_checks(&mut reporter, config_res).unwrap();
        reporter.finish().unwrap();
        let has_failures = reporter.has_failures();
        drop(reporter);
        (String::from_utf8(buf).unwrap(), has_failures)
    }

    #[test]
    fn tap_valid_config_emits_full_plan() {
        let config_res = Ok(config_with_one_agent());
        let (out, has_failures) = emit(HealthFormat::Tap, false, &config_res);
        assert!(!has_failures);
        assert!(out.starts_with("TAP version 14\n"), "got: {out}");
        assert!(out.contains("1..6"), "got: {out}");
        assert!(out.contains("ok 1 - config valid"), "got: {out}");
        assert!(out.contains("path: \"/tmp/test-config.toml\""), "got: {out}");
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
        assert!(out.contains("Bail out! configuration unusable"), "got: {out}");
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
        // plan + 6 tests + summary
        assert_eq!(lines.len(), 8);
    }

    #[test]
    fn ndjson_bad_config_bails_out() {
        let config_res = Err(color_eyre::eyre::eyre!("unknown field `not-a-real-key`"));
        let (out, has_failures) = emit(HealthFormat::Ndjson, false, &config_res);
        assert!(has_failures);
        assert!(out.contains("\"type\":\"bailout\""), "got: {out}");
        let summary: serde_json::Value =
            serde_json::from_str(out.lines().last().unwrap()).unwrap();
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
}
