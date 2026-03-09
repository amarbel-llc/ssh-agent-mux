
default: build test

build: build-nix build-rust

build-nix:
  nix build

build-rust:
  nix develop --command cargo build

dir_build := "target"

test: test-rust test-bats

test-rust:
  TMPDIR=/tmp nix develop --command tap-dancer cargo-test -skip-empty

test-bats: build-rust
  PATH="{{justfile_directory()}}/{{dir_build}}/debug:$PATH" just zz-tests_bats/test

reinstall-local: build-nix
  ./result/bin/ssh-agent-mux service uninstall
  ./result/bin/ssh-agent-mux service install

# Build from source, start isolated dev daemon, drop into shell with SSH_AUTH_SOCK
# pointing to the dev instance. Real upstream agents are still used.
dev: build-rust
  #!/usr/bin/env bash
  set -euo pipefail
  root="$(cd "{{justfile_directory()}}" && pwd)"
  build_dir="$root/{{dir_build}}/debug"
  binary="$build_dir/ssh-agent-mux"

  if [[ ! -x "$binary" ]]; then
    echo "binary not found: $binary" >&2
    exit 1
  fi

  dir=$(mktemp -d /tmp/ssh-agent-mux-dev-XXXXXX)
  trap 'kill "$daemon_pid" 2>/dev/null; wait "$daemon_pid" 2>/dev/null; rm -rf "$dir"' EXIT

  socket="$dir/agent.sock"
  log_file="$dir/ssh-agent-mux.log"
  config_dir="$dir/config/ssh-agent-mux"

  # Copy user's real config so we talk to the same upstream agents
  mkdir -p "$config_dir"
  src_config="${XDG_CONFIG_HOME:-$HOME/.config}/ssh-agent-mux/ssh-agent-mux.toml"
  if [[ -f "$src_config" ]]; then
    cp "$src_config" "$config_dir/ssh-agent-mux.toml"
  else
    echo "no config found at $src_config — generating default" >&2
    "$binary" config install --config "$config_dir/ssh-agent-mux.toml"
  fi

  # Override listen-path and log-file to use the temp dir
  # Use a temp file for portable sed -i
  tmp_sed=$(mktemp)
  sed \
    -e "s|^listen-path *=.*|listen-path = \"$socket\"|" \
    -e "s|^log-file *=.*|log-file = \"$log_file\"|" \
    -e "s|^log-level *=.*|log-level = \"debug\"|" \
    "$config_dir/ssh-agent-mux.toml" > "$tmp_sed"
  mv "$tmp_sed" "$config_dir/ssh-agent-mux.toml"

  # Start dev daemon
  XDG_CONFIG_HOME="$dir/config" "$binary" &
  daemon_pid=$!

  # Wait for socket
  deadline=$((SECONDS + 5))
  while [[ ! -S "$socket" ]] && [[ $SECONDS -lt $deadline ]]; do sleep 0.05; done
  if [[ ! -S "$socket" ]]; then
    echo "daemon failed to start within 5s" >&2
    echo "=== log ===" >&2
    cat "$log_file" 2>/dev/null >&2
    exit 1
  fi

  echo "dev ssh-agent-mux running (pid $daemon_pid)"
  echo "socket: $socket"
  echo "log:    $log_file"
  echo "config: $config_dir/ssh-agent-mux.toml"
  echo ""
  echo "SSH_AUTH_SOCK pointed at dev instance."
  echo "Try: ssh-add -l"
  echo ""

  # Drop into shell with SSH_AUTH_SOCK pointing to dev instance
  SSH_AUTH_SOCK="$socket" PATH="$build_dir:$PATH" "$SHELL"

# Build from source, start dev daemon using production config directly,
# with only socket and log paths isolated. Useful for testing against the
# exact config the system service uses.
dev-open: build-rust
  #!/usr/bin/env bash
  set -euo pipefail
  root="$(cd "{{justfile_directory()}}" && pwd)"
  build_dir="$root/{{dir_build}}/debug"
  binary="$build_dir/ssh-agent-mux"

  if [[ ! -x "$binary" ]]; then
    echo "binary not found: $binary" >&2
    exit 1
  fi

  dir=$(mktemp -d /tmp/ssh-agent-mux-dev-XXXXXX)
  trap 'kill "$daemon_pid" 2>/dev/null; wait "$daemon_pid" 2>/dev/null; rm -rf "$dir"' EXIT

  socket="$dir/agent.sock"
  log_file="$dir/ssh-agent-mux.log"

  # Start dev daemon using production config, overriding only socket and log
  "$binary" \
    --listen-path "$socket" \
    --log-level debug \
    --log-file "$log_file" &
  daemon_pid=$!

  # Wait for socket
  deadline=$((SECONDS + 5))
  while [[ ! -S "$socket" ]] && [[ $SECONDS -lt $deadline ]]; do sleep 0.05; done
  if [[ ! -S "$socket" ]]; then
    echo "daemon failed to start within 5s" >&2
    echo "=== log ===" >&2
    cat "$log_file" 2>/dev/null >&2
    exit 1
  fi

  echo "dev ssh-agent-mux running (pid $daemon_pid)"
  echo "socket: $socket"
  echo "log:    $log_file"
  echo "config: (production)"
  echo ""
  echo "SSH_AUTH_SOCK pointed at dev instance."
  echo "Try: ssh-add -l"
  echo ""

  # Drop into shell with SSH_AUTH_SOCK pointing to dev instance
  SSH_AUTH_SOCK="$socket" PATH="$build_dir:$PATH" "$SHELL"
