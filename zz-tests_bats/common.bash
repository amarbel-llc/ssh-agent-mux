bats_load_library bats-support
bats_load_library bats-assert
bats_load_library bats-assert-additions
bats_load_library bats-island

run_ssh_agent_mux() {
  run timeout --preserve-status "2s" ssh-agent-mux "$@"
}

write_config() {
  local config_dir="$XDG_CONFIG_HOME/ssh-agent-mux"
  mkdir -p "$config_dir"
  cat >"$config_dir/ssh-agent-mux.toml"
}
