use std::time::Duration;

use color_eyre::eyre::Result as EyreResult;
use ssh_agent_mux::MuxAgent;
use tokio::select;
use tokio::signal::{self, unix::SignalKind};

mod cli;
mod logging;
mod service;

#[cfg(debug_assertions)]
fn install_eyre_hook() -> EyreResult<()> {
    color_eyre::config::HookBuilder::default()
        .display_env_section(true)
        .install()
}

#[cfg(not(debug_assertions))]
fn install_eyre_hook() -> EyreResult<()> {
    color_eyre::config::HookBuilder::default()
        .display_env_section(false)
        .install()
}

// Use current_thread to keep our resource utilization down; this program will generally be
// accessed by only one user, at the start of each SSH session, so it doesn't need tokio's powerful
// async multithreading
#[tokio::main(flavor = "current_thread")]
async fn main() -> EyreResult<()> {
    install_eyre_hook()?;

    let mut config = cli::Config::parse()?;

    // Create parent directory for log file if it doesn't exist
    if let Some(ref log_file) = config.log_file {
        if let Some(parent) = log_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
    }

    // LoggerHandle must be held until program termination so file logging takes place
    let _logger = logging::setup_logger(config.log_level.into(), config.log_file.as_deref())?;

    if config.service.any() {
        return service::handle_service_command(&config);
    }

    // TODO: detect and remove stale socket before binding. If
    // listen_path exists but no process is listening (connect returns
    // ECONNREFUSED), unlink it so MuxAgent::run doesn't fail with
    // "Address already in use".

    let mut sigterm = signal::unix::signal(SignalKind::terminate())?;
    let mut sighup = signal::unix::signal(SignalKind::hangup())?;

    loop {
        let agent_paths = config.enabled_agent_socket_paths();
        let added_keys_path = config.added_keys_socket_path();
        select! {
            res = MuxAgent::run(&config.listen_path, &agent_paths, added_keys_path, Duration::from_secs(config.agent_timeout)) => { res?; break },
            // Cleanly exit on interrupt and SIGTERM, allowing
            // MuxAgent to clean up
            _ = signal::ctrl_c() => { log::info!("Exiting on SIGINT"); break },
            Some(_) = sigterm.recv() => { log::info!("Exiting on SIGTERM"); break },
            Some(_) = sighup.recv() => {
                log::info!("Reloading configuration");
                config = cli::Config::parse()?;
            }
        }
    }

    Ok(())
}
