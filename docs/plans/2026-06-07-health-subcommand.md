# `ssh-agent-mux health` Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use eng:subagent-driven-development to implement this plan task-by-task.

**Goal:** A `health` subcommand that runs the service/socket checks from the 2026-06-07 debugging session and emits TAP-14 text (NDJSON deferred to the upstream tap-dancer writer).

**Architecture:** New `health.rs` module in the binary; checks emit through a `HealthSink` trait backed by the upstream `tap-dancer` crate's `TapWriter`. Config parsing is split so `health` can report a broken config as a TAP failure instead of aborting. Service-manager state is read via `systemctl --user show` / unit-file presence; listener identity via `/proc/net/unix` + `/proc/<pid>/fd`; protocol probes via `ssh_agent_lib::client`.

**Tech Stack:** Rust (tokio current_thread, clap, ssh-agent-lib 0.6), `tap-dancer` git dep (amarbel-llc/tap, `rust/`), bats integration tests, nix `buildRustPackage` with `cargoLock.outputHashes`.

**Rollback:** Purely additive — revert the commits. No existing behavior changes except the `Config::parse` internal split (kept signature-compatible).

**Design doc:** `docs/plans/2026-06-07-health-subcommand-design.md` (approved). One temporary deviation, agreed during planning: until the upstream NDJSON writer lands, `--format auto` resolves to TAP text even on non-tty, and `--format ndjson` errors with "not yet supported". The design's tty-sniffing contract activates when NDJSON is wired in. \[Amendment 2026-06-07: tap-dancer v0.1.12 shipped the NDJSON writer + `Reporter` facade mid-implementation; the user approved folding it in. Plan-Task 4 (`HealthSink`/`TapTextSink`) is superseded by `Reporter`; plan-Task 5 wires it, retiring the deviation above.\]

**Conventions for every commit:** sign off as Clown per the global instructions. Do NOT run `just` before `merge-this-session` (the merge hook runs it). Cheap compile checks (`cargo build`) are fine. After adding new files, `git add` them before any `nix build` (dirty-tree nix builds only see tracked files).

______________________________________________________________________

## Task 1: Add the `tap-dancer` git dependency

**Files:**

- Modify: `Cargo.toml` (deps), `Cargo.lock`
- Modify: `flake.nix:162-166` (`cargoLock.outputHashes`)

**Step 1: Add the dependency**

Run: `nix develop --command cargo add tap-dancer --git https://github.com/amarbel-llc/tap.git`

Expected: Cargo resolves package `tap-dancer` v0.1.11 from the repo's `rust/` directory.

**Step 2: Verify it compiles and is usable**

Append a temporary doc-test-free smoke usage? No — just build: `nix develop --command cargo build`
Expected: success.

**Step 3: Update flake outputHashes**

`git add Cargo.toml Cargo.lock` (nix needs tracked files), then run `just build-nix`.
Expected: FAIL with a hash mismatch for `tap-dancer-0.1.11`, printing the correct `sha256-...`. Add that entry next to the existing `ssh-agent-lib-0.6.0` entry in `flake.nix` `cargoLock.outputHashes`, re-run `just build-nix`.
Expected: PASS.

**Step 4: Commit**

`git add flake.nix && git commit -m "build: add tap-dancer git dependency"`

______________________________________________________________________

## Task 2: Split config parsing so subcommands can see config errors

**Files:**

- Modify: `src/bin/ssh-agent-mux/cli.rs:158-191` (`Config::parse`)
- Test: existing `cargo test` suite (behavior-preserving refactor)

**Step 1: Refactor `Config::parse` into `parse_split`**

In `cli.rs`, replace the body of `parse()` with a delegation and add:

```rust
/// Parse CLI args, returning the subcommand and the config-load result
/// separately so subcommands (health) can render config failures
/// instead of aborting.
pub fn parse_split() -> (Option<service::Command>, EyreResult<Self>) {
    let mut args = Args::parse();
    let command = args.command;
    let config_res = (|| {
        let config_path = args.config_path.take().or_else(|| default_config_path().ok());
        let mut config = if let Some(ref path) = config_path {
            match File::open(path) {
                Ok(mut f) => {
                    log::info!("Read configuration from {}", path.display());
                    let mut config_text = String::new();
                    f.read_to_string(&mut config_text)?;
                    let expanded_config_text = expand_env_vars(&config_text)?;
                    let file_config =
                        toml::from_str::<<Config as ClapSerde>::Opt>(&expanded_config_text)?;
                    Config::from(file_config).merge(&mut args.config)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    Config::from(&mut args.config)
                }
                Err(e) => {
                    return Err(color_eyre::eyre::eyre!(
                        "Failed to read configuration from {}: {}",
                        path.display(),
                        e
                    ));
                }
            }
        } else {
            Config::from(&mut args.config)
        };
        config.config_path = config_path.unwrap_or_default();
        config.expand_and_validate()
    })();
    (command, config_res)
}

pub fn parse() -> EyreResult<(Self, Option<service::Command>)> {
    let (command, config) = Self::parse_split();
    Ok((config?, command))
}
```

Note: `service::Command` is `Clone + Copy`, so moving `args.command` out is fine; keep `parse()` because the SIGHUP-reload path (`main.rs:63`) uses it.

**Step 2: Verify behavior unchanged**

Run: `just test-rust`
Expected: PASS (pure refactor).

**Step 3: Commit**

`git commit -am "refactor(cli): split arg parsing from config loading"`

______________________________________________________________________

## Task 3: `Health` subcommand variant + dispatch skeleton

**Files:**

- Modify: `src/bin/ssh-agent-mux/service.rs:16-28` (Command enum)
- Create: `src/bin/ssh-agent-mux/health.rs`
- Modify: `src/bin/ssh-agent-mux/main.rs`
- Test: `zz-tests_bats/health.bats`

**Step 1: Write the failing bats test**

Create `zz-tests_bats/health.bats`:

```bash
#! /usr/bin/env bats

setup() {
  load "$(dirname "$BATS_TEST_FILE")/common.bash"
  setup_test_home
  export output
}

teardown() {
  teardown_test_home
}

function health_help_succeeds { # @test
  run_ssh_agent_mux health --help
  assert_success
  assert_output --partial "--format"
}

function health_ndjson_format_not_yet_supported { # @test
  run_ssh_agent_mux health --format ndjson
  assert_failure
  assert_output --partial "not yet supported"
}
```

**Step 2: Run to verify it fails**

Run: `just test-bats` (or `PATH="$PWD/target/debug:$PATH" just zz-tests_bats/test`)
Expected: FAIL — `health` is an unrecognized subcommand.

**Step 3: Implement the variant + stub**

`service.rs` — add to `Command` (keep `Clone, Copy`):

```rust
    /// Check agent and service health, emitting TAP
    Health {
        /// Output format (auto: TAP text on a tty; tap-ndjson otherwise once available)
        #[arg(long = "format", value_enum, default_value_t = crate::health::HealthFormat::Auto)]
        format: crate::health::HealthFormat,
    },
```

and in `handle_command`, `Command::Health { .. } => unreachable!("health is dispatched in main")`.

Create `health.rs`:

```rust
//! `ssh-agent-mux health`: TAP-emitting service + protocol health checks.
//!
//! Design: docs/plans/2026-06-07-health-subcommand-design.md

use clap_serde_derive::clap::ValueEnum;
use color_eyre::eyre::{Result, bail};

use crate::cli::Config;

#[derive(ValueEnum, Clone, Copy, PartialEq, Eq)]
pub enum HealthFormat {
    Auto,
    Tap,
    Ndjson,
}

pub async fn run(config_res: Result<Config>, format: HealthFormat) -> Result<()> {
    if format == HealthFormat::Ndjson {
        // Deviation from design, temporary: the workspace's single Rust
        // NDJSON producer is being added to the tap-dancer crate
        // (tap/clear-cherry session); wire it in when it lands. Until then
        // Auto always means TAP text.
        bail!("tap-ndjson output is not yet supported (pending upstream tap-dancer writer)");
    }
    let _ = config_res;
    todo!("checks arrive in later tasks")
}
```

`main.rs` — replace the parse + dispatch block:

```rust
mod health;
...
let (command, config_res) = cli::Config::parse_split();

// health owns stdout for its TAP stream and must see config errors
// itself, so dispatch before logger setup and config unwrap.
if let Some(service::Command::Health { format }) = command {
    return health::run(config_res, format).await;
}

let mut config = config_res?;
```

(keep the existing log-file dir creation, logger setup, and `handle_command` dispatch after this, unchanged).

**Step 4: Run tests**

Run: `just test-bats`
Expected: the two new tests PASS (help + ndjson error); everything else still green. `cargo build` first if needed.

**Step 5: Commit**

`git add -A && git commit -m "feat(health): subcommand skeleton with --format flag"`

______________________________________________________________________

## Task 4: `HealthSink` trait + TAP-text sink

**Files:**

- Create: `src/bin/ssh-agent-mux/health/sink.rs` (convert `health.rs` to `health/mod.rs`)
- Test: unit tests in `sink.rs`

**Step 1: Write the failing unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn render(f: impl FnOnce(&mut TapTextSink<'_, '_>)) -> String {
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut sink = TapTextSink::new(&mut buf).unwrap();
            f(&mut sink);
        }
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn ok_point_with_diagnostics_renders_comments() {
        let out = render(|s| {
            s.plan_ahead(1).unwrap();
            s.point(&Check {
                description: "listen socket answers".into(),
                status: CheckStatus::Ok,
                diagnostics: vec![("keys".into(), "3".into())],
            })
            .unwrap();
        });
        assert!(out.contains("TAP version 14"));
        assert!(out.contains("1..1"));
        assert!(out.contains("ok 1 - listen socket answers"));
        assert!(out.contains("# keys: 3"));
    }

    #[test]
    fn not_ok_point_renders_yaml_diagnostics_and_fails() {
        let out = render(|s| {
            s.plan_ahead(1).unwrap();
            s.point(&Check {
                description: "service active".into(),
                status: CheckStatus::NotOk,
                diagnostics: vec![("active-state".into(), "failed".into())],
            })
            .unwrap();
            assert!(s.has_failures());
        });
        assert!(out.contains("not ok 1 - service active"));
        assert!(out.contains("active-state: failed"));
    }

    #[test]
    fn skip_point_renders_directive_and_does_not_fail() {
        let out = render(|s| {
            s.plan_ahead(1).unwrap();
            s.point(&Check {
                description: "service installed".into(),
                status: CheckStatus::Skip("service not installed".into()),
                diagnostics: vec![],
            })
            .unwrap();
            assert!(!s.has_failures());
        });
        assert!(out.contains("# SKIP service not installed"));
    }
}
```

Set `NO_COLOR` is unnecessary: construct the writer with explicit `color(false)` (see Step 3) so tests are env-independent.

**Step 2: Run to verify failure**

Run: `nix develop --command cargo test --bin ssh-agent-mux health::`
Expected: FAIL to compile (types missing).

**Step 3: Implement**

`health/sink.rs`:

```rust
use std::io::{self, Write};

use tap_dancer::{TapWriter, TapWriterBuilder};

pub enum CheckStatus {
    Ok,
    NotOk,
    Skip(String),
}

pub struct Check {
    pub description: String,
    pub status: CheckStatus,
    /// key/value diagnostics; never affect ok/not-ok (key counts etc.)
    pub diagnostics: Vec<(String, String)>,
}

/// One emission interface so check code is agnostic of TAP text vs the
/// forthcoming upstream NDJSON writer (design doc, Output backends).
pub trait HealthSink {
    fn plan_ahead(&mut self, n: usize) -> io::Result<()>;
    fn point(&mut self, check: &Check) -> io::Result<()>;
    fn bail_out(&mut self, reason: &str) -> io::Result<()>;
    fn has_failures(&self) -> bool;
}

pub struct TapTextSink<'a, 'w> {
    writer: TapWriter<'w>,
    _marker: std::marker::PhantomData<&'a ()>,
}
```

(Implementor note: match the actual lifetimes `TapWriter<'a>` needs — it borrows the `dyn Write`. If the two-lifetime struct fights the borrow checker, own the writer construction inside `TapTextSink::new(w: &mut dyn Write)` returning `io::Result<TapTextSink<'_>>` with a single lifetime; the tests above only rely on `new(&mut Vec<u8>)`.)

```rust
impl<'w> TapTextSink<'w> {
    pub fn new(w: &'w mut dyn Write) -> io::Result<Self> {
        // no_locale + explicit color keeps output deterministic for tests;
        // run() decides color from tty-ness via TapWriterBuilder::auto later
        // if desired — start simple: color off.
        let writer = TapWriterBuilder::new(w).build()?;
        Ok(Self { writer })
    }
}

impl HealthSink for TapTextSink<'_> {
    fn plan_ahead(&mut self, n: usize) -> io::Result<()> {
        self.writer.plan_ahead(n)
    }

    fn point(&mut self, check: &Check) -> io::Result<()> {
        match &check.status {
            CheckStatus::Ok => {
                self.writer.ok(&check.description)?;
                for (k, v) in &check.diagnostics {
                    self.writer.comment(&format!("{k}: {v}"))?;
                }
            }
            CheckStatus::NotOk => {
                let diags: Vec<(&str, &str)> = check
                    .diagnostics
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                self.writer.not_ok_diag(&check.description, &diags)?;
            }
            CheckStatus::Skip(reason) => {
                self.writer.skip(&check.description, reason)?;
            }
        }
        Ok(())
    }

    fn bail_out(&mut self, reason: &str) -> io::Result<()> {
        self.writer.bail_out(reason)
    }

    fn has_failures(&self) -> bool {
        self.writer.has_failures()
    }
}
```

Convert `health.rs` → `health/mod.rs` with `mod sink; pub use sink::*;`.

**Step 4: Run tests**

Run: `nix develop --command cargo test --bin ssh-agent-mux health::`
Expected: PASS.

**Step 5: Commit**

`git add -A && git commit -m "feat(health): HealthSink trait + TAP text sink"`

______________________________________________________________________

## Task 5: Check engine with config check + placeholder skips

Wire `run()` end-to-end so the plan count and exit codes are final from
this task on; each later task replaces one placeholder
(`CheckStatus::Skip("not implemented")`) with a real check.

**Files:**

- Modify: `src/bin/ssh-agent-mux/health/mod.rs`
- Test: `zz-tests_bats/health.bats`

**Step 1: Write failing bats tests**

Append to `health.bats`:

```bash
function health_bad_config_bails_out { # @test
  write_config <<-EOF
	not-a-real-key = true
	EOF

  run_ssh_agent_mux health --format tap
  assert_failure
  assert_output --partial "not ok 1 - config valid"
  assert_output --partial "Bail out!"
}

function health_valid_config_emits_full_plan { # @test
  write_config <<-EOF
	[[agents]]
	name = "fake"
	socket-path = "/tmp/does-not-exist.sock"
	EOF

  run_ssh_agent_mux health --format tap
  assert_output --partial "TAP version 14"
  assert_output --partial "1..6"
  assert_output --partial "ok 1 - config valid"
}
```

**Step 2: Run to verify failure**

Run: `just test-bats`
Expected: new tests FAIL (todo!() panics).

**Step 3: Implement `run()`**

In `health/mod.rs`:

```rust
use std::io::Write;
use std::process;

pub async fn run(config_res: Result<Config>, format: HealthFormat) -> Result<()> {
    if format == HealthFormat::Ndjson {
        bail!("tap-ndjson output is not yet supported (pending upstream tap-dancer writer)");
    }
    // Auto currently always resolves to TAP text (see design doc deviation).

    let mut stdout = std::io::stdout().lock();
    let mut sink = TapTextSink::new(&mut stdout)?;
    let failed = emit_checks(&mut sink, config_res).await?;
    drop(sink);
    std::io::stdout().flush()?;
    if failed {
        process::exit(1);
    }
    Ok(())
}

const STATIC_CHECKS: usize = 5; // config, installed, active, held, answers

async fn emit_checks(sink: &mut dyn HealthSink, config_res: Result<Config>) -> Result<bool> {
    let config = match config_res {
        Err(e) => {
            sink.plan_ahead(1)?;
            sink.point(&Check {
                description: "config valid".into(),
                status: CheckStatus::NotOk,
                diagnostics: vec![("error".into(), format!("{e:#}"))],
            })?;
            sink.bail_out("configuration unusable")?;
            return Ok(true);
        }
        Ok(c) => c,
    };

    sink.plan_ahead(STATIC_CHECKS + config.agents.len())?;
    sink.point(&Check {
        description: "config valid".into(),
        status: CheckStatus::Ok,
        diagnostics: vec![
            ("path".into(), config.config_path.display().to_string()),
            ("agents".into(), config.agents.len().to_string()),
        ],
    })?;

    let placeholder = |desc: &str| Check {
        description: desc.into(),
        status: CheckStatus::Skip("not implemented".into()),
        diagnostics: vec![],
    };
    sink.point(&placeholder("service installed"))?;
    sink.point(&placeholder("service active"))?;
    sink.point(&placeholder("listen socket held by service"))?;
    sink.point(&placeholder("listen socket answers"))?;
    for agent in &config.agents {
        sink.point(&placeholder(&format!("upstream {} answers", agent.name)))?;
    }

    Ok(sink.has_failures())
}
```

**Step 4: Run tests**

Run: `just test-bats`
Expected: PASS (note `1..6` = 5 + 1 agent).

**Step 5: Commit**

`git add -A && git commit -m "feat(health): check engine, config check, plan + exit codes"`

______________________________________________________________________

## Task 6: Service-manager checks (installed, active)

**Files:**

- Modify: `src/bin/ssh-agent-mux/service.rs` (make `systemd_unit_dest()` — and on macOS the plist dest fn — `pub(crate)`)
- Create: `src/bin/ssh-agent-mux/health/service_state.rs`
- Test: unit tests in `service_state.rs`

**Step 1: Write failing unit tests for the systemctl parser**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_active_state_and_main_pid() {
        let out = "ActiveState=active\nMainPID=16891\n";
        let st = parse_systemctl_show(out);
        assert_eq!(st.active_state.as_deref(), Some("active"));
        assert_eq!(st.main_pid, Some(16891));
    }

    #[test]
    fn failed_unit_has_zero_main_pid() {
        let out = "ActiveState=failed\nMainPID=0\n";
        let st = parse_systemctl_show(out);
        assert_eq!(st.active_state.as_deref(), Some("failed"));
        assert_eq!(st.main_pid, None); // 0 normalized to None
    }
}
```

**Step 2: Run to verify failure** — `cargo test --bin ssh-agent-mux health::` → compile FAIL.

**Step 3: Implement**

`health/service_state.rs` (Linux path; macOS uses `launchctl print gui/$UID/net.ross-williams.ssh-agent-mux` exit status only, behind `#[cfg(target_os = ...)]`):

```rust
pub struct ServiceState {
    pub active_state: Option<String>,
    pub main_pid: Option<u32>,
}

pub fn parse_systemctl_show(out: &str) -> ServiceState {
    let mut active_state = None;
    let mut main_pid = None;
    for line in out.lines() {
        if let Some(v) = line.strip_prefix("ActiveState=") {
            active_state = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("MainPID=") {
            main_pid = v.trim().parse::<u32>().ok().filter(|p| *p != 0);
        }
    }
    ServiceState { active_state, main_pid }
}

/// None ⇒ systemctl unavailable (sandbox/CI) → caller skips the check.
pub fn query_service_state(unit: &str) -> Option<ServiceState> {
    let out = std::process::Command::new("systemctl")
        .args(["--user", "show", "-p", "ActiveState,MainPID", unit])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_systemctl_show(&String::from_utf8_lossy(&out.stdout)))
}
```

In `emit_checks`, replace the two placeholders:

- `service installed`: `service::systemd_unit_dest()` exists → Ok with `("unit", path)` diagnostic; missing → `Skip("service not installed")`; `unit_file_dir()`-style errors → Skip with the error string. Remember the skip so `active`/`held` emit `Skip("service not installed")` too.
- `service active`: `query_service_state(...)` `None` → `Skip("systemctl unavailable")`; `active_state == Some("active")` → Ok with `("main-pid", pid)`; else NotOk with `("active-state", state)`. Carry `main_pid` forward for Task 7.

**Step 4: Run tests** — `cargo test --bin ssh-agent-mux health::` PASS; `just test-bats` still green (sandbox → skips, plan unchanged).

**Step 5: Commit** — `git add -A && git commit -m "feat(health): service installed/active checks"`

______________________________________________________________________

## Task 7: Listener-identity check (Linux)

**Files:**

- Create: `src/bin/ssh-agent-mux/health/socket_holder.rs`
- Test: unit tests with fixture strings

**Step 1: Write failing unit tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const PROC_NET_UNIX: &str = "\
Num       RefCount Protocol Flags    Type St Inode Path
0000000000000000: 00000002 00000000 00010000 0001 01 72114 /home/sasha/.local/state/ssh/mux-agent.sock
0000000000000000: 00000002 00000000 00010000 0001 01 28168 /home/sasha/.local/state/ssh/pivy-agent.sock
";

    #[test]
    fn finds_inode_for_exact_path() {
        assert_eq!(
            unix_socket_inode(PROC_NET_UNIX, std::path::Path::new("/home/sasha/.local/state/ssh/mux-agent.sock")),
            Some(72114)
        );
    }

    #[test]
    fn missing_path_yields_none() {
        assert_eq!(unix_socket_inode(PROC_NET_UNIX, std::path::Path::new("/nope.sock")), None);
    }
}
```

**Step 2: Run to verify failure** — compile FAIL.

**Step 3: Implement**

```rust
/// Parse /proc/net/unix content; return the inode of the listening socket
/// bound at `path`. Column layout: ... Inode Path (Path may be absent).
pub fn unix_socket_inode(proc_net_unix: &str, path: &Path) -> Option<u64> {
    let want = path.to_str()?;
    proc_net_unix.lines().skip(1).find_map(|line| {
        let mut cols = line.split_whitespace();
        let inode = cols.by_ref().nth(6)?.parse::<u64>().ok()?;
        (cols.next()? == want).then_some(inode)
    })
}

/// True if /proc/<pid>/fd contains a link to socket:[inode].
pub fn pid_holds_socket_inode(pid: u32, inode: u64) -> bool {
    let needle = format!("socket:[{inode}]");
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .map(|entries| {
            entries.flatten().any(|e| {
                std::fs::read_link(e.path())
                    .map(|t| t.to_string_lossy() == needle)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Best-effort: scan /proc for the pid holding the socket (foreign-holder
/// diagnostic). Permission errors are skipped silently.
pub fn find_socket_holder(inode: u64) -> Option<u32> { /* iterate /proc/[0-9]+, reuse pid_holds_socket_inode */ }

pub fn pid_cgroup(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()
        .map(|s| s.lines().next().unwrap_or("").to_string())
}
```

Wire the `listen socket held by service` placeholder (all `#[cfg(target_os = "linux")]`; macOS emits `Skip("not implemented on macos")`):

- service skipped or no `main_pid` → `Skip("service not active")`
- inode not found → NotOk, `("error", "listen path not present in /proc/net/unix")`
- `pid_holds_socket_inode(main_pid, inode)` → Ok with `("main-pid", ...)`
- else NotOk with best-effort `("holder-pid", ...)`, `("holder-cgroup", ...)` from `find_socket_holder` — this is the line that names today's `ross-williams-ssh-agent-mux.service` failure mode.

**Step 4: Run tests** — unit PASS; bats still green (check skips in sandbox).

**Step 5: Commit** — `git commit -am "feat(health): listener-identity check via /proc"`

______________________________________________________________________

## Task 8: Protocol probes (listen socket + upstreams)

**Files:**

- Create: `src/bin/ssh-agent-mux/health/probe.rs`
- Test: async unit test binding a real `MuxAgent` (pattern: `src/lib.rs:622-671`)

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test(flavor = "current_thread")]
    async fn probe_counts_keys_of_live_agent() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("agent.sock");
        let agent = tokio::spawn(ssh_agent_mux::MuxAgent::run_for_test(sock.clone()));
        // (if no test constructor exists, spawn MuxAgent::run with empty
        // upstreams and poll for the socket file — mirror lib.rs tests)
        wait_for_socket(&sock).await;
        let n = probe_agent(&sock, Duration::from_secs(2)).await.unwrap();
        assert_eq!(n, 0); // mux with no upstreams answers with zero keys
        agent.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn probe_missing_socket_errors() {
        let err = probe_agent(std::path::Path::new("/nonexistent.sock"), Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(err.contains("No such file") || err.contains("connect"));
    }
}
```

**Step 2: Run to verify failure** — compile FAIL.

**Step 3: Implement**

```rust
use std::path::Path;
use std::time::Duration;

use ssh_agent_lib::client;

/// Connect to an agent socket and count its identities.
/// Mirrors the client path in examples/query-extensions.rs.
pub async fn probe_agent(path: &Path, timeout: Duration) -> Result<usize, String> {
    let fut = async {
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .map_err(|e| format!("connect {}: {e}", path.display()))?;
        let mut agent = client::connect(
            stream.into_std().map_err(|e| e.to_string())?.into(),
        )
        .map_err(|e| e.to_string())?;
        let ids = agent
            .request_identities()
            .await
            .map_err(|e| format!("request_identities: {e}"))?;
        Ok(ids.len())
    };
    tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| format!("timed out after {timeout:?}"))?
}
```

Wire the remaining placeholders in `emit_checks` (timeout = `Duration::from_secs(config.agent_timeout)`, the design's tuning lever):

- `listen socket answers`: probe `config.listen_path`; Ok + `("keys", n)` / NotOk + `("error", e)`.
- `upstream <name> answers`: per agent in config order; `!agent.enabled` → `Skip("disabled")`; else probe `agent.socket_path` the same way. Key counts never flip ok/not-ok.

**Step 4: Run tests** — `cargo test --bin ssh-agent-mux health::` PASS.

**Step 5: Commit** — `git commit -am "feat(health): protocol probes with key-count diagnostics"`

______________________________________________________________________

## Task 9: End-to-end bats coverage

**Files:**

- Test: `zz-tests_bats/health.bats`, helper in `zz-tests_bats/common.bash`

**Step 1: Add a daemon helper to common.bash**

```bash
# Background an ssh-agent-mux daemon on $1 with an empty-agent config,
# wait for the socket. PIDs collect in $STARTED_AGENTS for teardown.
start_fake_agent() {
  local sock="$1"
  XDG_CONFIG_HOME="$BATS_TEST_TMPDIR/empty-config" ssh-agent-mux --listen-path "$sock" &
  STARTED_AGENTS+=("$!")
  local deadline=$((SECONDS + 5))
  while [[ ! -S "$sock" && $SECONDS -lt $deadline ]]; do sleep 0.05; done
  [[ -S "$sock" ]]
}

stop_fake_agents() {
  for pid in "${STARTED_AGENTS[@]-}"; do kill "$pid" 2>/dev/null || true; done
}
```

(call `stop_fake_agents` from `health.bats` `teardown`.)

**Step 2: Write the failing tests**

```bash
function health_all_green_with_live_sockets { # @test
  start_fake_agent "$BATS_TEST_TMPDIR/upstream.sock"
  write_config <<-EOF
	listen-path = "$BATS_TEST_TMPDIR/listen.sock"

	[[agents]]
	name = "fake"
	socket-path = "$BATS_TEST_TMPDIR/upstream.sock"

	[[agents]]
	name = "off"
	socket-path = "/tmp/never.sock"
	enabled = false
	EOF
  # the mux under test, serving the configured listen path
  ssh-agent-mux &
  STARTED_AGENTS+=("$!")
  local deadline=$((SECONDS + 5))
  while [[ ! -S "$BATS_TEST_TMPDIR/listen.sock" && $SECONDS -lt $deadline ]]; do sleep 0.05; done

  run_ssh_agent_mux health --format tap
  assert_success
  assert_output --partial "1..7"
  assert_output --partial "ok 5 - listen socket answers"
  assert_output --partial "# keys: 0"
  assert_output --partial "ok 6 - upstream fake answers"
  assert_output --partial "# SKIP disabled"
}

function health_dead_upstream_fails { # @test
  write_config <<-EOF
	listen-path = "$BATS_TEST_TMPDIR/listen.sock"

	[[agents]]
	name = "gone"
	socket-path = "$BATS_TEST_TMPDIR/gone.sock"
	EOF

  run_ssh_agent_mux health --format tap
  assert_failure
  assert_output --partial "not ok 5 - listen socket answers"
  assert_output --partial "not ok 6 - upstream gone answers"
}
```

Note `run_ssh_agent_mux` wraps in `timeout 2s`; health against dead sockets must finish fast — pass a low timeout by adding `agent-timeout = 1` to the configs above if needed.

**Step 3: Run to verify** — `just test-bats`; iterate until green. Service checks 2–4 skip in the sandbox; assert their `# SKIP` lines too if stable.

**Step 4: Commit** — `git add -A && git commit -m "test(health): end-to-end bats coverage"`

______________________________________________________________________

## Task 10: Documentation

**Files:**

- Modify: `CLAUDE.md` (Architecture list: add `health.rs`)
- Modify: `README.md` (subcommand usage; mention TAP output + exit codes; note ndjson pending)

**Steps:** make both edits, then `git commit -am "docs: document health subcommand"`.

______________________________________________________________________

## Follow-ups (not in this plan)

- ~~Wire `--format ndjson` + tty auto-detection once `tap/clear-cherry` lands the upstream Rust NDJSON writer.~~ Obsolete: v0.1.12 shipped mid-plan and the wiring was folded into Task 5 (see the Design doc amendment above). No follow-up issue needed.
- Upstream wishlist already relayed: per-test `{number, description, ok, skip directive+reason, diagnostic map}`, plan-ahead, summary, common writer trait.
