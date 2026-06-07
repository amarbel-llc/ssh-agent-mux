bats_load_library bats-support
bats_load_library bats-assert
bats_load_library bats-assert-additions
bats_load_library bats-island
bats_load_library bats-emo

# Binary under test. The nix lane (flake.nix `bats-default`) exports
# SSH_AGENT_MUX_BIN pointing at the nix-built binary; local devshell runs
# fall back to PATH (e.g. target/debug via `just test-bats-local`).
# require_bin validates the var or the PATH fallback, then we pin the
# resolved absolute path so tests can also background "$SSH_AGENT_MUX_BIN" &
# directly and uniformly.
require_bin SSH_AGENT_MUX_BIN ssh-agent-mux
SSH_AGENT_MUX_BIN="${SSH_AGENT_MUX_BIN:-$(command -v ssh-agent-mux)}"
export SSH_AGENT_MUX_BIN

# Hermeticity: never let the host's real agent leak into tests. The nix
# lane's builder has no SSH_AUTH_SOCK; unsetting here keeps host runs
# identical. Tests that need one (e.g. `config install`) export their own
# deterministic value.
unset SSH_AUTH_SOCK

run_ssh_agent_mux() {
  run timeout --preserve-status "2s" "$SSH_AGENT_MUX_BIN" "$@"
}

write_config() {
  local config_dir="$XDG_CONFIG_HOME/ssh-agent-mux"
  mkdir -p "$config_dir"
  cat >"$config_dir/ssh-agent-mux.toml"
}
