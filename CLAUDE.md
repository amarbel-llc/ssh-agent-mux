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

### Bats: nix lane (authoritative) + host loop (iteration)

`just test-bats` builds the nix-sandboxed lane (`nix build .#bats-default`,
see bats-lane(7)): the suite runs hermetically against the nix-built binary
inside the nix builder. This is the gate the pre-merge hook exercises. Tests
resolve the binary via `$SSH_AGENT_MUX_BIN` (injected by the lane;
common.bash falls back to PATH for host runs). The builder has no network
and a scrubbed env — tests must not depend on host state (common.bash
unsets `SSH_AUTH_SOCK` for exactly this reason). New `.bats` files must be
git-tracked or the lane silently omits them.

`just test-bats-local` keeps the fast host loop (`bats --jobs N` against
`target/debug`) through the fence-sandboxing `bats` wrapper from the
`amarbel-llc/bats` flake input. Fence denies credential-dir reads
(`~/.ssh`, `~/.gnupg`, …) and network egress but allows
`socket(AF_UNIX, ...)`. Host fence can be flaky on some machines (bridge
init timeout); the nix lane is the fallback and the source of truth.
Wrapper flags worth knowing: `--no-sandbox` (bypass fence),
`--allow-local-binding`, `--no-tempdir-cleanup`. The pre-fence
`--allow-unix-sockets` flag no longer exists (fatal `Bad command line option`). Run `bats version` (positional) for wrapper/component versions.

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
