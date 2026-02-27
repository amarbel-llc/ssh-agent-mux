use std::{env, fs::File, io::Read, path::PathBuf};

use clap_serde_derive::{
    clap::{self, Parser, ValueEnum},
    serde::{self, Deserialize, Serialize},
    ClapSerde,
};
use color_eyre::eyre::Result as EyreResult;
use expand_tilde::ExpandTilde;
use log::LevelFilter;

use crate::service;

fn default_config_path() -> EyreResult<PathBuf> {
    let config_dir = env::var_os("XDG_CONFIG_HOME")
        .or_else(|| Some("~/.config".into()))
        .map(PathBuf::from)
        .and_then(|p| p.expand_tilde_owned().ok())
        .ok_or_else(|| color_eyre::eyre::eyre!("HOME not defined in environment"))?;

    Ok(config_dir
        .join(env!("CARGO_PKG_NAME"))
        .join(concat!(env!("CARGO_PKG_NAME"), ".toml")))
}


fn expand_env_vars(text: &str) -> EyreResult<String> {
    Ok(shellexpand::env(text)?.into_owned())
}

#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    /// Config file
    #[arg(short, long = "config")]
    config_path: Option<PathBuf>,

    /// Config from file or args
    #[command(flatten)]
    config: <Config as ClapSerde>::Opt,
}

fn default_enabled() -> bool {
    true
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct AgentConfig {
    pub name: String,
    pub socket_path: PathBuf,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(ClapSerde, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    /// Listen path
    #[default(PathBuf::from(concat!("~/.local/state/", env!("CARGO_PKG_NAME"), "/agent.sock")))]
    #[arg(long = "listen-path")]
    pub listen_path: PathBuf,

    /// Log level for agent
    #[default(LogLevel::Warn)]
    #[arg(long = "log-level", value_enum)]
    pub log_level: LogLevel,

    /// Optional log file for agent (logs to standard output, otherwise)
    #[arg(long = "log-file", num_args = 1)]
    pub log_file: Option<PathBuf>,

    /// Timeout in seconds for upstream agent operations (default: 5)
    #[default(5)]
    #[arg(long = "agent-timeout")]
    pub agent_timeout: u64,

    /// Upstream agents to multiplex
    #[arg(skip)]
    #[default(Vec::new())]
    pub agents: Vec<AgentConfig>,

    /// Name of agent to forward add_identity requests to
    #[arg(skip)]
    pub add_new_keys_to: Option<String>,

    // Following are part of command line args, but
    // not in configuration file
    /// Config file path (not an arg; copied from struct Args)
    #[arg(skip)]
    #[serde(skip_deserializing, skip_serializing)]
    pub config_path: PathBuf,

    #[serde(skip_deserializing, skip_serializing)]
    #[command(flatten)]
    pub service: service::ServiceArgs,
}

impl Config {
    pub fn parse() -> EyreResult<Self> {
        let mut args = Args::parse();

        let config_path = args.config_path.or_else(|| default_config_path().ok());

        let mut config = if let Some(ref path) = config_path {
            if let Ok(mut f) = File::open(path) {
                log::info!("Read configuration from {}", path.display());
                let mut config_text = String::new();
                f.read_to_string(&mut config_text)?;
                let expanded_config_text = expand_env_vars(&config_text)?;
                let file_config = toml::from_str::<<Config as ClapSerde>::Opt>(&expanded_config_text)?;
                Config::from(file_config).merge(&mut args.config)
            } else {
                Config::from(&mut args.config)
            }
        } else {
            Config::from(&mut args.config)
        };

        config.config_path = config_path.unwrap_or_default();
        config.listen_path = config.listen_path.expand_tilde_owned()?;
        config.log_file = config.log_file
            .map(|p| p.expand_tilde_owned())
            .transpose()?;
        config.agents = config
            .agents
            .into_iter()
            .map(|mut a| {
                a.socket_path = a.socket_path.expand_tilde_owned()?;
                Ok(a)
            })
            .collect::<EyreResult<Vec<_>>>()?;

        // Validate agent names are unique
        let mut seen_names = std::collections::HashSet::new();
        for agent in &config.agents {
            if !seen_names.insert(&agent.name) {
                return Err(color_eyre::eyre::eyre!(
                    "Duplicate agent name: {:?}",
                    agent.name
                ));
            }
        }

        // Validate add-new-keys-to references an existing, enabled agent
        if let Some(ref name) = config.add_new_keys_to {
            match config.agents.iter().find(|a| a.name == *name) {
                None => {
                    return Err(color_eyre::eyre::eyre!(
                        "add-new-keys-to references unknown agent: {:?}",
                        name
                    ));
                }
                Some(agent) if !agent.enabled => {
                    return Err(color_eyre::eyre::eyre!(
                        "add-new-keys-to references disabled agent: {:?}",
                        name
                    ));
                }
                _ => {}
            }
        }

        Ok(config)
    }

    pub fn enabled_agent_socket_paths(&self) -> Vec<PathBuf> {
        self.agents
            .iter()
            .filter(|a| a.enabled)
            .map(|a| a.socket_path.clone())
            .collect()
    }

    pub fn added_keys_socket_path(&self) -> Option<PathBuf> {
        self.add_new_keys_to.as_ref().and_then(|name| {
            self.agents
                .iter()
                .find(|a| a.name == *name)
                .map(|a| a.socket_path.clone())
        })
    }
}

#[derive(ValueEnum, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    #[value(hide = true)]
    Trace = 5,
}

impl From<LogLevel> for LevelFilter {
    fn from(value: LogLevel) -> Self {
        match value {
            LogLevel::Error => LevelFilter::Error,
            LogLevel::Warn => LevelFilter::Warn,
            LogLevel::Info => LevelFilter::Info,
            LogLevel::Debug => LevelFilter::Debug,
            LogLevel::Trace => LevelFilter::Trace,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_env_var_expansion() -> EyreResult<()> {
        // Test basic environment variable expansion
        env::set_var("TEST_VAR", "test_value");
        let result = expand_env_vars("${TEST_VAR}")?;
        assert_eq!(result, "test_value");

        // Test expansion in middle of string
        let result = expand_env_vars("/path/${TEST_VAR}/file")?;
        assert_eq!(result, "/path/test_value/file");

        // Test multiple variables
        env::set_var("TEST_VAR2", "another");
        let result = expand_env_vars("${TEST_VAR}_${TEST_VAR2}")?;
        assert_eq!(result, "test_value_another");

        Ok(())
    }

    #[test]
    fn test_config_with_env_vars() -> EyreResult<()> {
        use tempfile::NamedTempFile;

        // Set test environment variables
        env::set_var("TEST_HOME", "/test/home");
        env::set_var("TEST_USER", "testuser");
        env::set_var("TEST_SOCK", "/tmp/test.sock");

        // Create a temporary config file with environment variables
        let config_content = r#"
listen-path = "${TEST_HOME}/.ssh/agent-mux.sock"
log-level = "info"
log-file = "${TEST_HOME}/logs/ssh-agent-mux.log"

[[agents]]
name = "test-agent"
socket-path = "${TEST_SOCK}"

[[agents]]
name = "user-agent"
socket-path = "/tmp/ssh-agent-${TEST_USER}.sock"
"#;

        let mut temp_file = NamedTempFile::new()?;
        std::io::Write::write_all(&mut temp_file, config_content.as_bytes())?;

        // Test that our expansion function works on the config content
        let expanded = expand_env_vars(config_content)?;

        // Verify environment variables were expanded
        assert!(expanded.contains("/test/home/.ssh/agent-mux.sock"));
        assert!(expanded.contains("/test/home/logs/ssh-agent-mux.log"));
        assert!(expanded.contains("/tmp/test.sock"));
        assert!(expanded.contains("/tmp/ssh-agent-testuser.sock"));

        Ok(())
    }

    #[test]
    fn test_duplicate_agent_names_rejected() {
        let config_text = r#"
[[agents]]
name = "same"
socket-path = "/tmp/a.sock"

[[agents]]
name = "same"
socket-path = "/tmp/b.sock"
"#;

        let parsed = toml::from_str::<<Config as ClapSerde>::Opt>(config_text);
        assert!(parsed.is_ok(), "TOML should parse");

        let mut config = Config::from(parsed.unwrap());
        config.listen_path = "/tmp/test-listen.sock".into();

        let mut seen_names = std::collections::HashSet::new();
        let has_dupe = config.agents.iter().any(|a| !seen_names.insert(&a.name));
        assert!(has_dupe, "Should detect duplicate agent name");
    }

    #[test]
    fn test_invalid_add_new_keys_to_rejected() {
        let config_text = r#"
add-new-keys-to = "nonexistent"

[[agents]]
name = "real"
socket-path = "/tmp/a.sock"
"#;

        let parsed = toml::from_str::<<Config as ClapSerde>::Opt>(config_text).unwrap();
        let config = Config::from(parsed);

        let valid = config.add_new_keys_to.as_ref().map_or(true, |name| {
            config.agents.iter().any(|a| a.name == *name)
        });
        assert!(!valid, "Should reject reference to nonexistent agent");
    }

    #[test]
    fn test_enabled_filtering() {
        let config_text = r#"
[[agents]]
name = "active"
socket-path = "/tmp/active.sock"

[[agents]]
name = "disabled"
socket-path = "/tmp/disabled.sock"
enabled = false
"#;

        let parsed = toml::from_str::<<Config as ClapSerde>::Opt>(config_text).unwrap();
        let config = Config::from(parsed);

        let enabled_paths = config.enabled_agent_socket_paths();
        assert_eq!(enabled_paths.len(), 1);
        assert_eq!(enabled_paths[0], PathBuf::from("/tmp/active.sock"));
    }

    #[test]
    fn test_add_new_keys_to_resolution() {
        let config_text = r#"
add-new-keys-to = "target"

[[agents]]
name = "other"
socket-path = "/tmp/other.sock"

[[agents]]
name = "target"
socket-path = "/tmp/target.sock"
"#;

        let parsed = toml::from_str::<<Config as ClapSerde>::Opt>(config_text).unwrap();
        let config = Config::from(parsed);

        let resolved = config.added_keys_socket_path();
        assert_eq!(resolved, Some(PathBuf::from("/tmp/target.sock")));
    }
}
