# Single source of truth for ssh-agent-mux's config wire format: maps the
# structured (camelCase) options the Home Manager module exposes to the
# kebab-case keys the binary's serde config (`deny_unknown_fields`) expects.
#
# Imported by nix/home-manager.nix and by the `checks.<system>.config-render`
# flake check, which renders these settings to TOML and feeds them to
# `ssh-agent-mux config validate`. That makes any drift between these keys and
# the Rust `Config`/`AgentConfig` structs a build failure. Kept pkgs-free so it
# needs nothing beyond `lib`.
{ lib }:

# `cfg` is the module's `config.services.ssh-agent-mux` (or an equivalent
# attrset); only the fields read below matter, any extras are ignored.
cfg:
let
  # Nulls drop out so the binary falls back to its own defaults; the freeform
  # `settings` escape hatch is layered on top and wins.
  baseSettings = lib.filterAttrs (_: v: v != null) {
    listen-path = cfg.listenPath;
    log-level = cfg.logLevel;
    log-file = cfg.logFile;
    agent-timeout = cfg.agentTimeout;
    add-new-keys-to = cfg.addNewKeysTo;
    agents = map (a: {
      inherit (a) name enabled;
      socket-path = a.socketPath;
    }) cfg.agents;
  };
in
lib.recursiveUpdate baseSettings cfg.settings
