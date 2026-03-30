use std::{env, ffi::OsString, fmt::Write, fs, io, path::PathBuf, process};

use clap_serde_derive::clap::{self, Subcommand};
use color_eyre::{
    Section,
    eyre::{Result, WrapErr, bail, eyre},
};
use service_manager::{
    ServiceInstallCtx, ServiceManager, ServiceStartCtx, ServiceStatus, ServiceStatusCtx,
    ServiceStopCtx, ServiceUninstallCtx,
};

use crate::cli::Config;

const SERVICE_IDENT: &str = concat!("net.ross-williams.", env!("CARGO_PKG_NAME"));

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

fn handle_service_command(command: &ServiceCommand, config: &Config) -> Result<()> {
    let manager = {
        let mut m = <dyn ServiceManager>::native()?;
        if let Err(err) = m.set_level(service_manager::ServiceLevel::User) {
            if err.kind() == io::ErrorKind::Unsupported {
                return handle_set_level_error(command);
            } else {
                Err(err)?
            }
        }
        m
    };

    let label: service_manager::ServiceLabel =
        SERVICE_IDENT.parse().expect("SERVICE_IDENT is wrong");
    match command {
        ServiceCommand::Install => {
            if !config.config_path.try_exists()? {
                write_new_config_file(config)?;
            }
            Config::validate_file(&config.config_path)
                .wrap_err("config validation failed; service not installed")?;
            manager.install(ServiceInstallCtx {
                label: label.clone(),
                program: env::current_exe().note(concat!(
                    "Could not install service because path to ",
                    env!("CARGO_CRATE_NAME"),
                    " could not be determined."
                ))?,
                args: vec![
                    OsString::from("--config"),
                    config.config_path.as_os_str().to_owned(),
                ],
                contents: None,
                username: None,
                working_directory: None,
                environment: None,
                autostart: true,
                disable_restart_on_failure: false,
            })?;
            manager.start(ServiceStartCtx { label })?;
            println!("Installed and started service {}", SERVICE_IDENT);
        }
        ServiceCommand::Restart => {
            Config::validate_file(&config.config_path)
                .wrap_err("config validation failed; service not restarted")?;
            let status = manager.status(ServiceStatusCtx {
                label: label.clone(),
            })?;
            match status {
                ServiceStatus::Running => {
                    manager.stop(ServiceStopCtx {
                        label: label.clone(),
                    })?;
                }
                ServiceStatus::NotInstalled => {
                    bail!("Service {SERVICE_IDENT} not installed; can't restart");
                }
                ServiceStatus::Stopped(_) => (),
            }
            manager.start(ServiceStartCtx { label })?;
            println!("Restarted service {}", SERVICE_IDENT);
        }
        ServiceCommand::Uninstall => {
            manager.uninstall(ServiceUninstallCtx { label })?;
            println!("Uninstalled service {}", SERVICE_IDENT);
        }
    }

    Ok(())
}

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

    // Create parent directories if they don't exist
    if let Some(parent) = config.config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let new_config_toml = toml::to_string_pretty(&new_config)?;
    fs::write(&config.config_path, new_config_toml.as_bytes())?;
    Ok(())
}

fn handle_set_level_error(command: &ServiceCommand) -> Result<()> {
    let mut err = eyre!("Automatic management of a user service is unsupported on this platform");

    if matches!(command, ServiceCommand::Install) {
        let current_exe = env::current_exe().unwrap_or_else(|_| env!("CARGO_PKG_NAME").into());
        let current_exe_file_name = PathBuf::from(current_exe.file_name().unwrap());
        let arg0 = current_exe_file_name.display();
        err = err.suggestion(format!(
            r##"
To manually manage starting {arg0}, add the following to your shell startup script:

if ! ps -A -u "$(id -u)" | grep -q {arg0}; then
    {current_exe:?} > /dev/null &
fi"##
        ));
    }

    Err(err)
}

fn resolve_editor() -> Result<String> {
    env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .map_err(|_| {
            eyre!("Set VISUAL or EDITOR environment variable to use `ssh-agent-mux config edit`")
        })
}

fn restart_service_if_running() -> Result<()> {
    let manager = match <dyn ServiceManager>::native() {
        Ok(mut m) => {
            if let Err(err) = m.set_level(service_manager::ServiceLevel::User) {
                if err.kind() == io::ErrorKind::Unsupported {
                    println!("Service management not supported on this platform; skipping restart");
                    return Ok(());
                }
                return Err(err.into());
            }
            m
        }
        Err(_) => {
            println!("No service manager available; skipping restart");
            return Ok(());
        }
    };

    let label: service_manager::ServiceLabel =
        SERVICE_IDENT.parse().expect("SERVICE_IDENT is wrong");

    let status = manager.status(ServiceStatusCtx {
        label: label.clone(),
    })?;

    match status {
        ServiceStatus::Running => {
            manager.stop(ServiceStopCtx {
                label: label.clone(),
            })?;
            manager.start(ServiceStartCtx { label })?;
            println!("Restarted service {}", SERVICE_IDENT);
        }
        ServiceStatus::Stopped(_) => {
            println!("Service is stopped; skipping restart");
        }
        ServiceStatus::NotInstalled => {
            println!("Service not installed; skipping restart");
        }
    }

    Ok(())
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
        // Clean up temp file on editor failure
        let _ = fs::remove_file(&temp_path);
        bail!("Editor exited with status {}", code);
    }

    let edited_contents = fs::read(&temp_path)?;

    if original_contents == edited_contents {
        // temp_path is dropped here, which deletes the file
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

    // Write through any symlinks by opening the config path directly
    fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&config.config_path)
        .and_then(|mut f| io::Write::write_all(&mut f, &edited_contents))
        .wrap_err("Failed to write config file")?;

    println!("Updated config at {}", config.config_path.display());

    restart_service_if_running()
}
