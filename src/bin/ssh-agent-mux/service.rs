use std::{env, fmt::Write, fs, io, path::PathBuf, process};

use clap_serde_derive::clap::{self, Subcommand};
use color_eyre::eyre::{Result, WrapErr, bail, eyre};

use crate::cli::Config;

const SERVICE_LABEL: &str = concat!("net.ross-williams.", env!("CARGO_PKG_NAME"));

#[cfg(target_os = "macos")]
const PLIST_FILENAME: &str = concat!("net.ross-williams.", env!("CARGO_PKG_NAME"), ".plist");

#[cfg(target_os = "linux")]
const SYSTEMD_UNIT_FILENAME: &str = concat!(env!("CARGO_PKG_NAME"), ".service");

#[derive(Subcommand, Clone, Copy)]
pub enum Command {
    /// Manage the user service
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    /// Manage the configuration file
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand, Clone, Copy)]
pub enum ServiceCommand {
    /// Install the user service manager configuration and start the service
    Install,
    /// Stop and restart the user service
    Restart,
    /// Uninstall the user service manager configuration
    Uninstall,
}

#[derive(Subcommand, Clone, Copy)]
pub enum ConfigCommand {
    /// Generate a default configuration file
    Install,
    /// Validate the configuration file and print the resolved config
    Validate,
    /// Open the configuration file in $VISUAL or $EDITOR
    Edit,
}

pub fn handle_command(command: &Command, config: &Config) -> Result<()> {
    match command {
        Command::Config { command } => handle_config_command(command, config),
        Command::Service { command } => handle_service_command(command, config),
    }
}

fn handle_config_command(command: &ConfigCommand, config: &Config) -> Result<()> {
    match command {
        ConfigCommand::Edit => handle_edit_config(config),
        ConfigCommand::Validate => {
            let config_toml = toml::to_string_pretty(config)?;
            print!("{}", config_toml);
            Ok(())
        }
        ConfigCommand::Install => {
            if !config.config_path.try_exists()? {
                write_new_config_file(config)
            } else {
                bail!(
                    "Config file at {} already exists. Delete it and run `ssh-agent-mux config install` again if you want to re-generate",
                    config.config_path.display()
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Unit file location
// ---------------------------------------------------------------------------

/// Returns the directory containing pre-rendered unit files, located relative
/// to the resolved binary path (`../share/ssh-agent-mux/` from the binary).
///
/// Uses `current_exe()` which resolves symlinks, giving the real nix store
/// path. This ensures the unit files always match the binary that ships them.
fn unit_file_dir() -> Result<PathBuf> {
    let exe = env::current_exe().wrap_err("could not determine binary path")?;
    let bin_dir = exe
        .parent()
        .ok_or_else(|| eyre!("binary has no parent directory"))?;
    let pkg_dir = bin_dir
        .parent()
        .ok_or_else(|| eyre!("bin directory has no parent"))?;
    let dir = pkg_dir.join("share").join(env!("CARGO_PKG_NAME"));
    if !dir.is_dir() {
        bail!(
            "unit file directory not found at {}; service management requires the nix-built package",
            dir.display()
        );
    }
    Ok(dir)
}

fn home_dir() -> Result<PathBuf> {
    env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| eyre!("HOME is not set"))
}

// ---------------------------------------------------------------------------
// macOS (launchd)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn plist_source() -> Result<PathBuf> {
    Ok(unit_file_dir()?.join(PLIST_FILENAME))
}

#[cfg(target_os = "macos")]
fn plist_dest() -> Result<PathBuf> {
    Ok(home_dir()?
        .join("Library/LaunchAgents")
        .join(PLIST_FILENAME))
}

#[cfg(target_os = "macos")]
fn run_launchctl(args: &[&str]) -> Result<()> {
    let output = process::Command::new("launchctl")
        .args(args)
        .output()
        .wrap_err("failed to execute launchctl")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "launchctl {} failed (exit {}): {}",
            args.first().unwrap_or(&""),
            output.status.code().unwrap_or(-1),
            stderr.trim()
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn service_install(config: &Config) -> Result<()> {
    if !config.config_path.try_exists()? {
        write_new_config_file(config)?;
    }
    Config::validate_file(&config.config_path)
        .wrap_err("config validation failed; service not installed")?;

    let source = plist_source()?;
    let dest = plist_dest()?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&source, &dest).wrap_err_with(|| {
        format!(
            "failed to copy plist from {} to {}",
            source.display(),
            dest.display()
        )
    })?;

    // Unload any existing instance (ignore errors — may not be loaded)
    let _ = run_launchctl(&["remove", SERVICE_LABEL]);
    run_launchctl(&["load", &dest.to_string_lossy()])?;

    println!("Installed and started service {SERVICE_LABEL}");
    Ok(())
}

#[cfg(target_os = "macos")]
fn service_restart(config: &Config) -> Result<()> {
    Config::validate_file(&config.config_path)
        .wrap_err("config validation failed; service not restarted")?;

    let _ = run_launchctl(&["stop", SERVICE_LABEL]);
    run_launchctl(&["start", SERVICE_LABEL])?;

    println!("Restarted service {SERVICE_LABEL}");
    Ok(())
}

#[cfg(target_os = "macos")]
fn service_uninstall() -> Result<()> {
    let _ = run_launchctl(&["remove", SERVICE_LABEL]);

    let dest = plist_dest()?;
    if dest.try_exists()? {
        fs::remove_file(&dest)?;
    }

    println!("Uninstalled service {SERVICE_LABEL}");
    Ok(())
}

#[cfg(target_os = "macos")]
fn restart_service_if_running() -> Result<()> {
    // launchctl stop + start; if the service isn't loaded, stop will fail and
    // we just skip the restart.
    if run_launchctl(&["stop", SERVICE_LABEL]).is_ok() {
        run_launchctl(&["start", SERVICE_LABEL])?;
        println!("Restarted service {SERVICE_LABEL}");
    } else {
        println!("Service not loaded; skipping restart");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Linux (systemd)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn systemd_unit_source() -> Result<PathBuf> {
    Ok(unit_file_dir()?.join(SYSTEMD_UNIT_FILENAME))
}

#[cfg(target_os = "linux")]
fn systemd_unit_dest() -> Result<PathBuf> {
    let config_dir = env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().expect("HOME is not set").join(".config"));
    Ok(config_dir.join("systemd/user").join(SYSTEMD_UNIT_FILENAME))
}

#[cfg(target_os = "linux")]
fn run_systemctl(args: &[&str]) -> Result<()> {
    let output = process::Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .wrap_err("failed to execute systemctl")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "systemctl --user {} failed (exit {}): {}",
            args.first().unwrap_or(&""),
            output.status.code().unwrap_or(-1),
            stderr.trim()
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn service_install(config: &Config) -> Result<()> {
    if !config.config_path.try_exists()? {
        write_new_config_file(config)?;
    }
    Config::validate_file(&config.config_path)
        .wrap_err("config validation failed; service not installed")?;

    let source = systemd_unit_source()?;
    let dest = systemd_unit_dest()?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&source, &dest).wrap_err_with(|| {
        format!(
            "failed to copy unit from {} to {}",
            source.display(),
            dest.display()
        )
    })?;

    run_systemctl(&["daemon-reload"])?;
    run_systemctl(&["enable", "--now", SYSTEMD_UNIT_FILENAME])?;

    println!("Installed and started service {SERVICE_LABEL}");
    Ok(())
}

#[cfg(target_os = "linux")]
fn service_restart(config: &Config) -> Result<()> {
    Config::validate_file(&config.config_path)
        .wrap_err("config validation failed; service not restarted")?;

    run_systemctl(&["restart", SYSTEMD_UNIT_FILENAME])?;

    println!("Restarted service {SERVICE_LABEL}");
    Ok(())
}

#[cfg(target_os = "linux")]
fn service_uninstall() -> Result<()> {
    let _ = run_systemctl(&["disable", "--now", SYSTEMD_UNIT_FILENAME]);
    run_systemctl(&["daemon-reload"])?;

    let dest = systemd_unit_dest()?;
    if dest.try_exists()? {
        fs::remove_file(&dest)?;
    }

    println!("Uninstalled service {SERVICE_LABEL}");
    Ok(())
}

#[cfg(target_os = "linux")]
fn restart_service_if_running() -> Result<()> {
    if run_systemctl(&["is-active", "--quiet", SYSTEMD_UNIT_FILENAME]).is_ok() {
        run_systemctl(&["restart", SYSTEMD_UNIT_FILENAME])?;
        println!("Restarted service {SERVICE_LABEL}");
    } else {
        println!("Service not running; skipping restart");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Unsupported platforms
// ---------------------------------------------------------------------------

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn service_install(_config: &Config) -> Result<()> {
    unsupported_platform_error()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn service_restart(_config: &Config) -> Result<()> {
    unsupported_platform_error()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn service_uninstall() -> Result<()> {
    unsupported_platform_error()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn restart_service_if_running() -> Result<()> {
    println!("Service management not supported on this platform; skipping restart");
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn unsupported_platform_error() -> Result<()> {
    let bin = env!("CARGO_PKG_NAME");
    let exe = env::current_exe()
        .map(|p| format!("{:?}", p))
        .unwrap_or_else(|_| bin.to_string());
    bail!(
        r##"Automatic management of a user service is unsupported on this platform.

To manually manage starting {bin}, add the following to your shell startup script:

if ! ps -A -u "$(id -u)" | grep -q {bin}; then
    {exe} > /dev/null &
fi"##
    );
}

// ---------------------------------------------------------------------------
// Service command dispatch
// ---------------------------------------------------------------------------

fn handle_service_command(command: &ServiceCommand, config: &Config) -> Result<()> {
    match command {
        ServiceCommand::Install => service_install(config),
        ServiceCommand::Restart => service_restart(config),
        ServiceCommand::Uninstall => service_uninstall(),
    }
}

// ---------------------------------------------------------------------------
// Config file management (unchanged)
// ---------------------------------------------------------------------------

fn write_new_config_file(config: &Config) -> Result<()> {
    let mut success_msg = format!(
        "Automatically creating configuration file at {} ",
        config.config_path.display()
    );

    let mut new_config = config.clone();
    if config.agents.is_empty() {
        match env::var("SSH_AUTH_SOCK") {
            Ok(v) => {
                success_msg.write_str("with the current SSH_AUTH_SOCK as the upstream agent; please edit to add additional agents.")?;
                new_config.agents.push(crate::cli::AgentConfig {
                    name: "default".into(),
                    socket_path: v.into(),
                    enabled: true,
                });
            }
            Err(e) => {
                let mut emsg = String::from("A new configuration file cannot be created: ");
                match e {
                    env::VarError::NotPresent => {
                        emsg.write_str("SSH_AUTH_SOCK is not in the environment, and no upstream agent paths were specified on the command line.")?;
                    }
                    env::VarError::NotUnicode(_) => {
                        emsg.write_str(
                            "SSH_AUTH_SOCK is defined, but contains non-UTF-8 characters.",
                        )?;
                    }
                }
                bail!(emsg);
            }
        };
    } else {
        write!(
            success_msg,
            "with the upstream agent socket paths specified in config."
        )?;
    }

    println!("{}", success_msg);

    if let Some(parent) = config.config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let new_config_toml = toml::to_string_pretty(&new_config)?;
    fs::write(&config.config_path, new_config_toml.as_bytes())?;
    Ok(())
}

fn resolve_editor() -> Result<String> {
    env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .map_err(|_| {
            eyre!("Set VISUAL or EDITOR environment variable to use `ssh-agent-mux config edit`")
        })
}

fn handle_edit_config(config: &Config) -> Result<()> {
    if !config.config_path.try_exists()? {
        bail!(
            "No config file found at {}; run `ssh-agent-mux config install` first",
            config.config_path.display()
        );
    }

    let editor = resolve_editor()?;

    let original_contents = fs::read(&config.config_path)?;

    let temp_file = tempfile::Builder::new()
        .prefix("ssh-agent-mux-")
        .suffix(".toml")
        .tempfile()
        .wrap_err("Failed to create temporary file")?;

    let temp_path = temp_file.into_temp_path();
    fs::write(&temp_path, &original_contents)?;

    let status = process::Command::new(&editor)
        .arg(&temp_path)
        .status()
        .wrap_err_with(|| format!("Failed to launch editor: {}", editor))?;

    if !status.success() {
        let code = status
            .code()
            .map_or("unknown".to_string(), |c| c.to_string());
        let _ = fs::remove_file(&temp_path);
        bail!("Editor exited with status {}", code);
    }

    let edited_contents = fs::read(&temp_path)?;

    if original_contents == edited_contents {
        println!("No changes made");
        return Ok(());
    }

    if let Err(err) = Config::validate_file(&temp_path) {
        let kept_path = temp_path
            .keep()
            .map_err(|e| eyre!("Failed to persist temp file: {}", e.error))?;
        return Err(err).wrap_err(format!(
            "Validation failed; your edits are saved at {}",
            kept_path.display()
        ));
    }

    fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&config.config_path)
        .and_then(|mut f| io::Write::write_all(&mut f, &edited_contents))
        .wrap_err("Failed to write config file")?;

    println!("Updated config at {}", config.config_path.display());

    restart_service_if_running()
}
