# CLAUDE.md

## Overview

ssh-agent-mux is a Rust binary that multiplexes keys from multiple SSH agents
into a single agent socket. It uses tokio (single-threaded runtime) for async
I/O and signal handling.

## Build & Test

```sh
just build          # nix build + cargo build
just test           # cargo test + bats integration tests
just test-rust      # cargo test only
just test-bats      # bats integration tests only
```

## Testing

### Bats sandbox (fence-based)

Tests run through the `bats` wrapper from the `amarbel-llc/bats` flake input,
which sandboxes each test command with `fence` (not seccomp/sandcastle). The
fence config denies reads of credential dirs (`~/.ssh`, `~/.gnupg`, …) and all
network egress, but does **not** block `socket(AF_UNIX, ...)`, so the mux's
Unix-socket I/O and tokio's signal handlers work without any extra flag.

An older sandcastle-based wrapper required `--allow-unix-sockets`; that flag is
**not** accepted by the current fence-based wrapper (it errors `Bad command line option`) and has been removed from `zz-tests_bats/justfile`. Wrapper flags
worth knowing: `--no-sandbox` (bypass fence), `--allow-local-binding`,
`--no-tempdir-cleanup`. Run `bats version` (positional) for wrapper/component
versions.

## Architecture

- `src/bin/ssh-agent-mux/main.rs` --- Entry point, tokio runtime, signal
  handling
- `src/bin/ssh-agent-mux/cli.rs` --- Config parsing (TOML + CLI args + env var
  expansion)
- `src/bin/ssh-agent-mux/service.rs` --- Subcommands (service
  install/restart/uninstall, config install/validate/edit)
- `src/lib.rs` --- `MuxAgent` core logic (SSH agent protocol multiplexing)

## Configuration

TOML config at `~/.config/ssh-agent-mux/ssh-agent-mux.toml`. Supports
environment variable expansion (`${VAR}`) and tilde expansion in paths. Uses
`deny_unknown_fields` --- typos in config keys are rejected.
