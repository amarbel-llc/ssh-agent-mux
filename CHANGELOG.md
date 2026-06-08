# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.2] - 2026-06-08

### Added
- Add tap-dancer git dependency
- Add debug-service-health + debug-probe-sockets recipes
- Add Home Manager module

### Changed
- Tighten the health section per editorial review
- Complete the ndjson record inventory + locale pragma note
- Document health subcommand
- Pilot conformist alongside treefmt (ssh-agent-mux#16)
- Protocol probes with key-count diagnostics
- Bump tap-dancer to v0.1.13 (skip_diag + ndjson Drop safety net)
- Listener-identity check via /proc
- Service installed/active checks
- Rustfmt reflow in the ndjson bail-out test
- Check engine via Reporter (tap, ndjson, auto)
- Mark the plan's ndjson follow-up obsolete
- Bump tap-dancer to v0.1.12 (ndjson writer + Reporter facade)
- Mdformat reflow of the CLAUDE.md testing section
- CLAUDE.md testing section reflects the nix bats lane
- Run bats suite as a nix lane (bats-default)
- Expose batman manpages on MANPATH
- Subcommand skeleton with --format flag
- Split arg parsing from config loading
- Mdformat the health-subcommand plan docs
- Implementation plan for `health` subcommand
- Design for `health` subcommand with TAP output

### Fixed
- Harden e2e bats helpers per review
- End-to-end bats coverage with live daemons
- Concurrent upstream probes, current_thread test flavor
- Structural holder invariant + precise no-pid skip reason
- Bound service-manager queries, honest launchctl state
- Locale-free forced-tap output + test hardening
- Bail instead of panic on the unimplemented default path
- Faithful Home Manager module eval check
- Reclaim stale listen socket before bind

## [0.1.0] - 2026-05-29

- Add bats binary to the devShell
- Add query-extensions diagnostic + confirm-query-aggregation recipe
- Add repo-level sweatfile gating pre-merge on full TAP version 14
pragma +streamed-output
ok 1 - suite-1 # SKIP no tests
    # Subtest: suite-2
    pragma +streamed-output
    ok 1 - cli::tests::test_config_with_env_vars
    ok 2 - cli::tests::test_add_new_keys_to_resolution
    ok 3 - cli::tests::test_enabled_filtering
    ok 4 - cli::tests::test_duplicate_agent_names_rejected
    ok 5 - cli::tests::test_env_var_expansion
    ok 6 - cli::tests::test_invalid_add_new_keys_to_rejected
    ok 7 - cli::tests::test_unknown_config_keys_rejected
    ok 8 - cli::tests::test_unknown_agent_keys_rejected
    1..8
ok 2 - suite-2
    # Subtest: suite-3
    pragma +streamed-output
    ok 1 - edit_config_no_changes
    ok 2 - edit_config_invalid_change_preserves_original
    ok 3 - edit_config_no_editor_set
    ok 4 - edit_config_missing_config
    ok 5 - edit_config_editor_failure
    ok 6 - edit_config_valid_change
    ok 7 - edit_config_preserves_symlink
    1..7
ok 3 - suite-3
    # Subtest: suite-4
    pragma +streamed-output
    ok 1 - query_response_decodes_flat_openssh_cstrings
    ok 2 - pivy_like_agent_advertises_extensions_via_query
    ok 3 - echo_agent_handles_custom_extension
    ok 4 - mux_query_response_includes_upstream_extensions
    ok 5 - mux_forwards_unknown_extensions_to_upstream
    ok 6 - mux_query_response_aggregates_multiple_upstreams
    1..6
ok 4 - suite-4
    # Subtest: suite-5
    pragma +streamed-output
    ok 1 - empty_mux_agent
    ok 2 - mux_add_identity_forwarding
    ok 3 - mux_with_one_agent
    ok 4 - add_keys_to_openssh_agent
    ok 5 - mux_add_identity_constrained_forwarding
    ok 6 - mux_with_three_agents
    ok 7 - mux_lock_unlock_multiple_agents
    ok 8 - mux_lock_unlock
    1..8
ok 5 - suite-5
ok 6 - suite-6 # SKIP no tests
1..6
{"type":"summary","passed":11,"failed":0,"skipped":0,"todo":0,"total":11,"plan_count":11,"bailed":false,"valid":true,"diagnostics":[]}
- Add CLAUDE.md with bats sandbox unix socket gotcha
- Add outputHash for git-sourced ssh-agent-lib dependency
- Add dev and dev-open recipes for isolated local testing
- Add feature design record for forwarding constrained add-identity requests
- Add tap-dancer for cargo test output and to devShell
- Add multi-agent and add-new-keys-to config validation tests
- Add BATS integration test infrastructure
- Add edit-config design docs and TODO for editor splitting
- Add --edit-config flag to ServiceArgs
- Add CI workflow
- Add better error reporting to integration tests
- Add --log-file option
- Add homebrew to release CI
- Add configuration reloading on SIGHUP
- Add some trace logging
- Add integration test
- Add --install-config option
- Add dependabot configuration
- Add color-eyre and improve some error reporting
- Add release workflow and shell script
- Add tag message to cliff.toml
- Add documentation to public functions and structs
- Add git-cliff configuration

- Format tree with treefmt
- Adopt eng justfile, treefmt & release conventions
- Bump ssh-agent-lib to 0.6.0 (master), adapt to credential rename
- Consume batman/tap-dancer from bats+tap, drop bob
- Bump version to 0.2.0
- Apply formatting (cargo fmt, yaml indent, shfmt)
- Switch tap-dancer from purse-first to bob flake input
- Forward constrained add-identity requests to upstream agent
- Replace utility flags with grouped subcommands
- Implement --edit-config command
- Move tempfile to regular dependency for edit-config
- Update README.md config example
- Bump tempfile from 3.19.1 to 3.20.0 by @dependabot[bot]
- Bump tokio from 1.44.2 to 1.45.0 by @dependabot[bot]
- Bump duct from 0.13.7 to 1.0.0 by @dependabot[bot]
- Bump toml from 0.8.21 to 0.8.22 by @dependabot[bot]
- Bump toml from 0.8.20 to 0.8.21 by @dependabot[bot]
- Cargo fmt
- Clean up quoting in homebrew formula generation
- Switch homebrew-releaser CI back to upstream
- Switch fork of homebrew-releaser to main branch
- Update Homebrew tap repository name
- Test homebrew-releaser local changes
- Move test harness into separate module
- Cargo fmt
- Automatic configuration file generation
- Cargo update
- Dependabot only manages upstream dependencies
- Suggest how to configure on service-unsupported platforms
- Move main and modules to a separate bin directory
- Extract logging module
- Service management (as described in README)
- Cargo fmt
- Simplify parsing of tilde in upstream agent paths
- Tilde (HOME) expansion in configuration
- Update dependencies
- Prepare README.md, LICENSEs, etc.
- Refactor session-bind error handling
- Refactor MuxAgentSession
- Improve logging configuration
- Refactor session-bind@openssh.com extension support
- Update to ssh-agent-lib 0.5.1
- Cleared known keys after every upstream agent
- Non-working session-bind extension handling
- Implement basic sign functionality

- Gate release on master, the actual default branch
- Drop obsolete --allow-unix-sockets flag
- Pin flat OpenSSH query-response decode (ssh-agent-mux#10)
- Render launchd/systemd unit files at nix build time instead of runtime
- Update ssh-agent-lib to v0.5.2 (main branch) for query wire format fix
- Use argv[0] for service install to preserve symlink paths
- Aggregate upstream agent extensions in query response
- Forward unknown SSH agent extensions to upstream agents
- Allow unix sockets in bats sandbox for tokio signal handling
- Upgrade Rust to edition 2024, replace purse-first devenv with rust-overlay
- Filter out certificate identities during refresh_identities
- Log full error variant chain instead of lossy Display output
- Temporarily disable cross qemu tests
- Update MSRV to 1.81.0
- Fix line length in release workflow
- Fix homebrew-releaser workflow
- Fix homebrew-tap workflow step
- Correct error handling for session-bind@openssh.com extension
- Correct extension query response; handle unsupported extension
- Refactor known_keys lock to hold lock across signing request
- Make logging filtering work better with flexi-logger crate

- Remove certificate identity filtering workaround

* @overhacked made their first contribution
* @dependabot[bot] made their first contribution

### Added
- Add bats binary to the devShell
- Add query-extensions diagnostic + confirm-query-aggregation recipe
- Add repo-level sweatfile gating pre-merge on full `just`
- Add CLAUDE.md with bats sandbox unix socket gotcha
- Add outputHash for git-sourced ssh-agent-lib dependency
- Add dev and dev-open recipes for isolated local testing
- Add feature design record for forwarding constrained add-identity requests
- Add tap-dancer for cargo test output and to devShell
- Add multi-agent and add-new-keys-to config validation tests
- Add BATS integration test infrastructure
- Add edit-config design docs and TODO for editor splitting
- Add --edit-config flag to ServiceArgs
- Add CI workflow
- Add better error reporting to integration tests
- Add --log-file option
- Add homebrew to release CI
- Add configuration reloading on SIGHUP
- Add some trace logging
- Add integration test
- Add --install-config option
- Add dependabot configuration
- Add color-eyre and improve some error reporting
- Add release workflow and shell script
- Add tag message to cliff.toml
- Add documentation to public functions and structs
- Add git-cliff configuration

### Changed
- Format tree with treefmt
- Adopt eng justfile, treefmt & release conventions
- Bump ssh-agent-lib to 0.6.0 (master), adapt to credential rename
- Consume batman/tap-dancer from bats+tap, drop bob
- Bump version to 0.2.0
- Apply formatting (cargo fmt, yaml indent, shfmt)
- Switch tap-dancer from purse-first to bob flake input
- Forward constrained add-identity requests to upstream agent
- Replace utility flags with grouped subcommands
- Implement --edit-config command
- Move tempfile to regular dependency for edit-config
- Update README.md config example
- Bump tempfile from 3.19.1 to 3.20.0 by @dependabot[bot]
- Bump tokio from 1.44.2 to 1.45.0 by @dependabot[bot]
- Bump duct from 0.13.7 to 1.0.0 by @dependabot[bot]
- Bump toml from 0.8.21 to 0.8.22 by @dependabot[bot]
- Bump toml from 0.8.20 to 0.8.21 by @dependabot[bot]
- Cargo fmt
- Clean up quoting in homebrew formula generation
- Switch homebrew-releaser CI back to upstream
- Switch fork of homebrew-releaser to main branch
- Update Homebrew tap repository name
- Test homebrew-releaser local changes
- Move test harness into separate module
- Cargo fmt
- Automatic configuration file generation
- Cargo update
- Dependabot only manages upstream dependencies
- Suggest how to configure on service-unsupported platforms
- Move main and modules to a separate bin directory
- Extract logging module
- Service management (as described in README)
- Cargo fmt
- Simplify parsing of tilde in upstream agent paths
- Tilde (HOME) expansion in configuration
- Update dependencies
- Prepare README.md, LICENSEs, etc.
- Refactor session-bind error handling
- Refactor MuxAgentSession
- Improve logging configuration
- Refactor session-bind@openssh.com extension support
- Update to ssh-agent-lib 0.5.1
- Cleared known keys after every upstream agent
- Non-working session-bind extension handling
- Implement basic sign functionality

### Fixed
- Gate release on master, the actual default branch
- Drop obsolete --allow-unix-sockets flag
- Pin flat OpenSSH query-response decode (ssh-agent-mux#10)
- Render launchd/systemd unit files at nix build time instead of runtime
- Update ssh-agent-lib to v0.5.2 (main branch) for query wire format fix
- Use argv[0] for service install to preserve symlink paths
- Aggregate upstream agent extensions in query response
- Forward unknown SSH agent extensions to upstream agents
- Allow unix sockets in bats sandbox for tokio signal handling
- Upgrade Rust to edition 2024, replace purse-first devenv with rust-overlay
- Filter out certificate identities during refresh_identities
- Log full error variant chain instead of lossy Display output
- Temporarily disable cross qemu tests
- Update MSRV to 1.81.0
- Fix line length in release workflow
- Fix homebrew-releaser workflow
- Fix homebrew-tap workflow step
- Correct error handling for session-bind@openssh.com extension
- Correct extension query response; handle unsupported extension
- Refactor known_keys lock to hold lock across signing request
- Make logging filtering work better with flexi-logger crate

### Removed
- Remove certificate identity filtering workaround

[0.1.2]: https://github.com/overhacked/ssh-agent-mux/compare/v0.1.1..v0.1.2

<!-- generated by git-cliff -->
