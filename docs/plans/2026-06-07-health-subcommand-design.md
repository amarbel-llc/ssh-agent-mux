# Design: `ssh-agent-mux health` subcommand

- **Date:** 2026-06-07
- **Status:** approved
- **Origin:** debugging session where the Home Manager unit failed on boot
  with `EADDRINUSE` because a leftover foreign unit
  (`ross-williams-ssh-agent-mux.service`) held the listen socket. The
  diagnosis required cross-referencing `systemctl`, `journalctl`, `ss -xlp`,
  `/proc`, and per-socket `ssh-add -l` probes (now captured as the
  `debug-service-health` / `debug-probe-sockets` justfile recipes). This
  subcommand turns that investigation into a one-shot, machine-readable
  health check.

## Goals

- One command that checks both layers we debugged: service-manager state
  (unit installed/active, socket ownership) and agent-protocol state
  (sockets answer, key counts).
- TAP output: classic TAP-14 text on a tty, tap-ndjson(7) otherwise.
- Generic over configured upstreams — no agent-specific (e.g. piggy)
  knowledge.
- Exit code usable by scripts: 0 = healthy, 1 = any failing check.

## Non-goals

- Reading journals or enumerating competing units (stays in the justfile
  debug recipes).
- Judging key *presence*: an upstream that answers with zero keys is
  healthy; counts are diagnostics only.
- A second NDJSON producer in the workspace (see Output backends).

## CLI surface

- `ssh-agent-mux health` — new `Health` variant alongside `Service`/`Config`
  in `service::Command`.
- `--format <auto|tap|ndjson>`, default `auto` (tty → `tap`, non-tty →
  `ndjson`). Explicit values exist so non-tty tests can reach the text
  renderer and pty-bound agents can reach NDJSON.
- Exit: 0 all pass (skips do not fail), 1 any `not ok` or bail-out.

## Check plan

Flat top-level TAP test points; plan emitted up front
(count = 5 + len(agents), deterministic from config). Descriptions are a
stable contract for scripts.

| # | description | semantics |
|---|---|---|
| 1 | `config valid` | TOML parsed + validated; diagnostic: config path, agent count |
| 2 | `service installed` | unit file at `systemd_unit_dest()` (Linux) / plist at launchd dest (macOS); **skip** with reason if absent → 3–4 auto-skip |
| 3 | `service active` | `systemctl --user show -p ActiveState,MainPID` / `launchctl print`; skip with reason if the tool itself is unavailable (sandbox/CI) |
| 4 | `listen socket held by service` | Linux: listen path's inode from `/proc/net/unix` present in `/proc/<MainPID>/fd`; mismatch → `not ok` + best-effort foreign-holder pid/cgroup diagnostic; macOS: skip |
| 5 | `listen socket answers` | connect + `request_identities` via `ssh_agent_lib::client`; diagnostic: aggregate key count |
| 6..N | `upstream <name> answers` | one per configured agent, config order; disabled → skip with reason `disabled`; diagnostic: key count (never affects ok) |

Check 4 is the one today's incident needed: it renders the foreign-holder
case as a single `not ok` naming the holding pid, instead of two lines
(`not ok - service active` + `ok - listen socket answers`) that must be
correlated by hand.

## Config-failure path

Today `cli::Config::parse()?` aborts before subcommand dispatch, so a
typo'd TOML (`deny_unknown_fields` rejection) could never reach `health`.
Change: clap-parse args first; when the subcommand is `health`, config-load
errors are caught and rendered as `not ok 1 - config valid` + `Bail out!` +
summary, exit 1. Other subcommands keep current behavior.

## Output backends

Both writers come from the upstream `tap-dancer` crate
(`amarbel-llc/tap`, package at `rust/`), consumed as a git dependency —
one new `cargoLock.outputHashes` entry in flake.nix, same pattern as
`ssh-agent-lib`:

- **TAP text:** existing `TapWriter` (shipped, v0.1.11).
- **TAP-NDJSON:** the writer being added by the `tap/clear-cherry` session
  (explicitly for the ssh-agent-mux + piggy health commands). Not yet
  committed as of this design.

ssh-agent-mux's check code emits through one thin internal trait
(`HealthSink`: plan-ahead, test point `{n, description, ok, skip-reason,
yaml-diagnostics}`, summary) with two impls delegating to the upstream
writers — unless upstream ships its own writer-agnostic interface, in
which case theirs is used and the trait disappears.

**Sequencing:** implement `health` fully against TAP text now;
`--format ndjson` (and the non-tty auto path) returns a clear
"not yet supported" error until the upstream writer lands, then wiring it
is a small follow-up (bump the git dep pin + impl). No local NDJSON
emitter — exactly one Rust producer in the workspace.

## Error handling

Each check is isolated: a failing probe yields `not ok` + error-string
diagnostic and the run continues. Connect/probe timeout reuses
`agent_timeout` from config. Only unrecoverable internal errors (e.g.
stdout write failure) bail out.

## Testing

- Unit: check-to-test-point mapping; sink trait round-trips.
- Bats (zz-tests_bats): fake upstream agent sockets via the existing
  harness; run `health --format tap` (and `ndjson` once it exists), assert
  with grep/jq: summary counts, skip reasons, key-count diagnostics, exit
  codes. Service-manager checks naturally skip inside the fence sandbox
  (no systemctl) — which also exercises the skip path.

## Rollback

Additive feature; nothing replaced. Rollback = revert the commit(s). The
justfile debug recipes remain as the deeper service-manager diagnostics.

## Tuning levers

- **Probe timeout** — currently `agent_timeout` (default 5 s). Change
  signal: health probes hang or false-timeout in practice.
- **Foreign-holder scan** — full `/proc` walk, best-effort, only on
  check-4 failure. Change signal: noticeable latency on busy hosts → cap
  the walk.
- **Format auto-detection** — tty sniffing on stdout. Change signal: more
  consumers need explicit modes than the flag covers.

## Coordination

- `tap/clear-cherry` (upstream NDJSON writer): wishlist relayed via Sasha —
  per-test `{number, description, ok, skip directive+reason, diagnostic
  map}`, plan-ahead, summary, and ideally a common trait over both writers.
- Follow-up after upstream lands: pin bump + NDJSON impl + bats lane.
