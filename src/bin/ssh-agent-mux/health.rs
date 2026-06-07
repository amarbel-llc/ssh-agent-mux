//! `ssh-agent-mux health`: TAP-emitting service + protocol health checks.
//!
//! Design: docs/plans/2026-06-07-health-subcommand-design.md

use clap_serde_derive::clap::{self, ValueEnum};
use color_eyre::eyre::{Result, bail};

use crate::cli::Config;

#[derive(ValueEnum, Clone, Copy, PartialEq, Eq)]
pub enum HealthFormat {
    Auto,
    Tap,
    Ndjson,
}

pub async fn run(config_res: Result<Config>, format: HealthFormat) -> Result<()> {
    if format == HealthFormat::Ndjson {
        // Deviation from design, temporary: the workspace's single Rust
        // NDJSON producer is being added to the tap-dancer crate
        // (tap/clear-cherry session); wire it in when it lands. Until then
        // Auto always means TAP text.
        bail!("tap-ndjson output is not yet supported (pending upstream tap-dancer writer)");
    }
    let _ = config_res;
    todo!("checks arrive in later tasks")
}
