default: lint build test

dir_build := "target"

# Read-only formatting gate (treefmt via the `checks.formatting` derivation).
[group('pre-build')]
lint-fmt:
  #!/usr/bin/env bash
  set -euo pipefail
  # Builds checks.formatting, which runs treefmt against a /nix/store snapshot
  # and fails if anything would change. Does NOT touch the worktree --- the
  # modifying counterpart is `codemod-fmt-treefmt`.
  system=$(nix eval --raw --impure --expr 'builtins.currentSystem')
  nix build ".#checks.${system}.formatting" --no-link --print-build-logs

lint: lint-fmt

[group('build')]
build-nix:
  nix build

[group('build')]
build-rust:
  nix develop --command cargo build

build: build-nix build-rust

[group('post-build')]
test-rust:
  TMPDIR=/tmp nix develop --command tap-dancer cargo-test -skip-empty

[group('post-build')]
test-bats: build-rust
  PATH="{{justfile_directory()}}/{{dir_build}}/debug:$PATH" just zz-tests_bats/test

test: test-rust test-bats

# Format the whole tree in place with treefmt (`nix fmt`); gate is `lint-fmt`.
[group('codemod')]
codemod-fmt-treefmt:
  nix fmt

codemod-fmt: codemod-fmt-treefmt

# Reinstall the system service from a fresh nix build.
[group('operational')]
install-local: build-nix
  ./result/bin/ssh-agent-mux service uninstall
  ./result/bin/ssh-agent-mux service install

# Run an isolated dev daemon (copy of your real config) on a temp socket, then open a shell.
[group('operational')]
run-dev: build-rust
  #!/usr/bin/env bash
  set -euo pipefail
  # Real upstream agents are still used; only the listen socket and logs are
  # isolated to a temp dir.
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

# Run a dev daemon using your production config, isolating only socket + log paths.
[group('operational')]
run-dev-open: build-rust
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

# Bump the version in version.env (canonical) and Cargo.toml's [package] version.
[group('maint')]
bump-version new_version:
  # Pure mutation: staging/committing is `release`'s job. The Cargo.toml edit
  # is scoped to the [package] table so it never touches the [dependencies.*]
  # version lines, which also sit at column 0.
  sed -E -i 's/^(export SSH_AGENT_MUX_VERSION)=.*/\1={{new_version}}/' version.env
  sed -E -i '/^\[package\]/,/^\[/ s/^version = "[^"]*"/version = "{{new_version}}"/' Cargo.toml

# Create, push, and verify a signed `v<sem>` tag read from version.env.
[group('maint')]
tag message:
  #!/usr/bin/env bash
  set -euo pipefail
  . version.env
  tag="v${SSH_AGENT_MUX_VERSION:?missing SSH_AGENT_MUX_VERSION in version.env}"
  git tag -s -m "{{message}}" "$tag"
  gum log --level info "Created tag: $tag"
  git push origin "$tag"
  gum log --level info "Pushed $tag"
  git tag -v "$tag"

# Full release: changelog, version bump, commit, signed tag, and GitHub release.
[group('maint')]
release new_version:
  #!/usr/bin/env bash
  set -euo pipefail

  # Release only from the default branch.
  branch=$(git rev-parse --abbrev-ref HEAD)
  if [[ "$branch" != "main" ]]; then
    gum log --level error "release only allowed from main (on '$branch')"
    exit 1
  fi

  tag="v{{new_version}}"

  # Generate the changelog BEFORE bump-version. git-cliff skips the
  # chore(release) commit, so the bump never lands in the notes.
  git cliff --tag "$tag" --output CHANGELOG.md
  notes=$(git cliff --tag "$tag" --unreleased --strip all)

  just bump-version "{{new_version}}"
  # Rust-specific: cargo records the crate version in Cargo.lock too, so
  # rebuild to keep the lock in step with the bumped Cargo.toml.
  nix develop --command cargo build
  git add version.env Cargo.toml Cargo.lock CHANGELOG.md
  git commit -m "chore(release): $tag"

  just tag "$notes"

  # gh release create is the publication step; CI on tag push is verify-only.
  gh release create "$tag" --title "$tag" --notes "$notes"

# Stand up a throwaway mux vs your real upstreams; dump aggregated query extensions (ssh-agent-mux#10).
[group('debug')]
debug-query-aggregation: build-rust
  #!/usr/bin/env bash
  set -euo pipefail
  # Non-disruptive: uses a temp socket and never touches the installed service.
  # Prints each upstream's and the mux's advertised `query` extensions via the
  # query-extensions example.
  root="$(cd "{{justfile_directory()}}" && pwd)"
  build_dir="$root/{{dir_build}}/debug"
  binary="$build_dir/ssh-agent-mux"

  nix develop --command cargo build --quiet --example query-extensions
  probe="$build_dir/examples/query-extensions"

  src_config="${XDG_CONFIG_HOME:-$HOME/.config}/ssh-agent-mux/ssh-agent-mux.toml"
  if [[ ! -f "$src_config" ]]; then
    echo "no config found at $src_config" >&2
    exit 1
  fi

  dir=$(mktemp -d /tmp/ssh-agent-mux-confirm-XXXXXX)
  trap 'kill "${daemon_pid:-}" 2>/dev/null; wait "${daemon_pid:-}" 2>/dev/null; rm -rf "$dir"' EXIT

  socket="$dir/agent.sock"
  config_dir="$dir/config/ssh-agent-mux"
  mkdir -p "$config_dir"

  # Reuse the real config so we talk to the real upstream agents, overriding
  # only the listen path to a throwaway socket.
  sed -e "s|^listen-path *=.*|listen-path = \"$socket\"|" \
    "$src_config" > "$config_dir/ssh-agent-mux.toml"

  XDG_CONFIG_HOME="$dir/config" "$binary" &
  daemon_pid=$!

  deadline=$((SECONDS + 5))
  while [[ ! -S "$socket" ]] && [[ $SECONDS -lt $deadline ]]; do sleep 0.05; done
  if [[ ! -S "$socket" ]]; then
    echo "throwaway mux failed to start within 5s" >&2
    exit 1
  fi

  echo "=== upstream agents (from $src_config) ==="
  grep -E '^socket-path' "$src_config" \
    | sed -E 's/^socket-path *= *"(.*)"/\1/' \
    | while read -r up; do
        up_expanded="${up/#\$HOME/$HOME}"
        up_expanded="${up_expanded/#\~/$HOME}"
        echo "--- $up_expanded ---"
        "$probe" "$up_expanded" || echo "(probe failed)"
      done

  echo "=== mux aggregated (listen=$socket) ==="
  "$probe" "$socket"
