# CLAUDE.md

## Overview

ssh-agent-mux is a Rust binary that multiplexes keys from multiple SSH agents
into a single agent socket. It uses tokio (single-threaded runtime) for async
I/O and signal handling.

## Build & Test

``` sh
just build          # nix build + cargo build
just test           # cargo test + bats integration tests
just test-rust      # cargo test only
just test-bats      # bats integration tests only
```

## Testing

### Bats sandbox requires `--allow-unix-sockets`

Batman's `bats` wrapper runs tests inside sandcastle, which blocks
`socket(AF_UNIX, ...)` via seccomp by default. Because tokio registers Unix
signal handlers at runtime startup (before any user code), this causes every
invocation to panic with `PermissionDenied` --- even `--help` and `--version`.

The `zz-tests_bats/justfile` passes `--allow-unix-sockets` to `bats` to permit
this. If you see panics like:

    failed to create UnixStream: Os { code: 1, kind: PermissionDenied, message: "Operation not permitted" }

The fix is `--allow-unix-sockets` on the `bats` command, **not** changing the
Rust code or Claude Code sandbox settings.

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
