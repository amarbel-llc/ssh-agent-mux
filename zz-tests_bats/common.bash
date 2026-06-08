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

# --- Daemon-backed tests (health e2e) ---
#
# Helpers to background ssh-agent-mux daemons inside a test. PIDs collect
# in STARTED_AGENTS; call stop_fake_agents from teardown in any file that
# uses these. Daemon stdout/stderr append to $BATS_TEST_TMPDIR/daemons.log
# so a misbehaving daemon can be diagnosed from the preserved tmpdir
# (`--no-tempdir-cleanup`) or by cat-ing the log into test output.

# Wait up to ~5s for a unix socket to appear at $1; fails the caller if
# it never does.
wait_for_socket() {
  local sock="$1"
  local deadline=$((SECONDS + 5))
  while [[ ! -S $sock && $SECONDS -lt $deadline ]]; do sleep 0.05; done
  [[ -S $sock ]]
}

# Background an ssh-agent-mux serving $1 with an empty-agent config (the
# isolated XDG_CONFIG_HOME has no config file, so defaults + the
# --listen-path override apply): a stand-in upstream agent that answers
# request_identities with zero keys.
start_fake_agent() {
  local sock="$1"
  XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/empty-config" "$SSH_AGENT_MUX_BIN" \
    --listen-path "$sock" >>"$BATS_TEST_TMPDIR/daemons.log" 2>&1 &
  STARTED_AGENTS+=("$!")
  wait_for_socket "$sock"
}

stop_fake_agents() {
  local pid
  for pid in "${STARTED_AGENTS[@]-}"; do kill "$pid" 2>/dev/null || true; done
  STARTED_AGENTS=()
}
