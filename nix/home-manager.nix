# Home Manager module for ssh-agent-mux.
#
# Exposed from the flake as `homeManagerModules.ssh-agent-mux` (and
# `.default`). It writes the TOML config to
# `~/.config/ssh-agent-mux/ssh-agent-mux.toml` and, by default, runs the mux as
# a user service (systemd `--user` on Linux, launchd agent on macOS).
#
# Unlike `ssh-agent-mux service install`, this manages the unit declaratively
# through Home Manager, so the imperative `service`/`config` subcommands are not
# needed when using this module.
self:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.ssh-agent-mux;

  tomlFormat = pkgs.formats.toml { };

  # Structured options -> the kebab-case keys the binary's serde config expects.
  # Nulls are dropped so they fall back to the binary's own defaults, and any
  # `settings` escape-hatch keys are layered on top. The whole mapping is
  # exercised end-to-end against the binary by the `checks.home-manager-module`
  # flake check (ssh-agent-mux#13).
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

  configFile = tomlFormat.generate "ssh-agent-mux.toml" (
    lib.recursiveUpdate baseSettings cfg.settings
  );

  agentNames = map (a: a.name) cfg.agents;
  enabledAgentNames = map (a: a.name) (lib.filter (a: a.enabled) cfg.agents);

  agentModule = lib.types.submodule {
    options = {
      name = lib.mkOption {
        type = lib.types.str;
        example = "1password";
        description = "Unique name identifying this upstream agent.";
      };

      socketPath = lib.mkOption {
        type = lib.types.str;
        example = "~/.1password/agent.sock";
        description = ''
          Path to the upstream agent's listening socket. Supports `~` and
          `''${VAR}` expansion, performed by ssh-agent-mux itself.
        '';
      };

      enabled = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Whether keys from this agent are offered through the mux.";
      };
    };
  };
in
{
  options.services.ssh-agent-mux = {
    enable = lib.mkEnableOption "ssh-agent-mux, an SSH agent multiplexer";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "ssh-agent-mux.packages.\${system}.default";
      description = "The ssh-agent-mux package to use.";
    };

    listenPath = lib.mkOption {
      type = lib.types.str;
      default = "${config.xdg.stateHome}/ssh-agent-mux/agent.sock";
      defaultText = lib.literalExpression ''"''${config.xdg.stateHome}/ssh-agent-mux/agent.sock"'';
      description = ''
        Socket path that ssh-agent-mux listens on. Point your SSH client's
        `SSH_AUTH_SOCK` / `IdentityAgent` at this path (or set
        {option}`services.ssh-agent-mux.enableSshAuthSock`).
      '';
    };

    agents = lib.mkOption {
      type = lib.types.listOf agentModule;
      default = [ ];
      example = lib.literalExpression ''
        [
          {
            name = "1password";
            socketPath = "~/.1password/agent.sock";
          }
          {
            name = "yubikey";
            socketPath = "~/.ssh/yubikey-agent.sock";
          }
        ]
      '';
      description = ''
        Upstream SSH agents to multiplex. The order keys are offered to a server
        follows the order of this list.
      '';
    };

    addNewKeysTo = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "1password";
      description = ''
        Name of the agent (from {option}`services.ssh-agent-mux.agents`) that
        `ssh-add` / add-identity requests are forwarded to. When `null`, adding
        keys through the mux fails.
      '';
    };

    logLevel = lib.mkOption {
      type = lib.types.enum [
        "error"
        "warn"
        "info"
        "debug"
        "trace"
      ];
      default = "warn";
      description = "Log verbosity for ssh-agent-mux.";
    };

    logFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "~/.local/state/ssh-agent-mux/agent.log";
      description = "Optional file to log to. Logs to stdout when `null`.";
    };

    agentTimeout = lib.mkOption {
      type = lib.types.ints.unsigned;
      default = 5;
      description = "Timeout, in seconds, for upstream agent operations.";
    };

    installService = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether to run ssh-agent-mux as a user service (systemd `--user` on
        Linux, a launchd agent on macOS). Disable to only manage the config file
        and package.
      '';
    };

    enableSshAuthSock = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Whether to export `SSH_AUTH_SOCK` (pointing at
        {option}`services.ssh-agent-mux.listenPath`) as a session variable so
        SSH clients use the mux by default.
      '';
    };

    settings = lib.mkOption {
      type = tomlFormat.type;
      default = { };
      example = lib.literalExpression ''
        {
          log-level = "info";
        }
      '';
      description = ''
        Extra settings merged into the generated TOML config, using the binary's
        kebab-case keys. Takes precedence over the structured options above; use
        as an escape hatch for keys this module does not expose yet.
      '';
    };
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        assertions = [
          {
            assertion = lib.length (lib.unique agentNames) == lib.length agentNames;
            message = "services.ssh-agent-mux.agents: agent names must be unique.";
          }
          {
            assertion = cfg.addNewKeysTo == null || lib.elem cfg.addNewKeysTo enabledAgentNames;
            message =
              "services.ssh-agent-mux.addNewKeysTo must reference the name of an "
              + "enabled agent in services.ssh-agent-mux.agents.";
          }
        ];

        home.packages = [ cfg.package ];

        xdg.configFile."ssh-agent-mux/ssh-agent-mux.toml".source = configFile;

        home.sessionVariables = lib.mkIf cfg.enableSshAuthSock {
          SSH_AUTH_SOCK = cfg.listenPath;
        };
      }

      (lib.mkIf (cfg.installService && pkgs.stdenv.hostPlatform.isLinux) {
        systemd.user.services.ssh-agent-mux = {
          Unit = {
            Description = "SSH Agent Multiplexer";
            After = [ "graphical-session-pre.target" ];
          };
          Service = {
            ExecStart = "${cfg.package}/bin/ssh-agent-mux --config ${configFile}";
            ExecReload = "${pkgs.coreutils}/bin/kill -HUP $MAINPID";
            Restart = "on-failure";
          };
          Install.WantedBy = [ "default.target" ];
        };
      })

      (lib.mkIf (cfg.installService && pkgs.stdenv.hostPlatform.isDarwin) {
        launchd.agents.ssh-agent-mux = {
          enable = true;
          config = {
            ProgramArguments = [
              "${cfg.package}/bin/ssh-agent-mux"
              "--config"
              "${configFile}"
            ];
            KeepAlive = true;
            RunAtLoad = true;
          };
        };
      })
    ]
  );
}
