# `ssh-agent-mux` - Combine keys from multiple SSH agents into a single agent socket

Numerous types of SSH agents exist, such as the [1Password SSH agent](https://developer.1password.com/docs/ssh/agent/), which allows access to private keys in shared vaults, or [yubikey-agent](https://github.com/FiloSottile/yubikey-agent), allowing seamless access to private keys stored on [YubiKey](https://www.yubico.com/products/) cryptography devices. The `ssh` command allows using only one agent at-a-time, requiring you to configure per-server [`IdentityAgent`](https://www.mankier.com/5/ssh_config#IdentityAgent) settings or change the `SSH_AUTH_SOCK` environment variable depending on which agent you wish to use.

`ssh-agent-mux` combines multiple agents' keys into a single agent, allowing you to configure an SSH client just once. Provide all "upstream" SSH agents' `SSH_AUTH_SOCK` paths in the `ssh-agent-mux` [configuration](#configuration) and [run](#usage) `ssh-agent-mux` via your login scripts or OS's user service manager. Point your SSH configuration at `ssh-agent-mux`'s socket, and it will offer all available public keys from upstream agents as available for authentication.

## Features

- Simple TOML configuration syntax
- [systemd](https://systemd.io/) and [launchd](https://en.wikipedia.org/wiki/Launchd) user service manager integration
- [`session-bind@openssh.com` extension](https://github.com/openssh/openssh-portable/blob/46e52fdae08b89264a0b23f94391c2bf637def34/PROTOCOL.agent) pass-through support for agents that support key usage constraints

## Roadmap

- Background daemon support for running directly from the command line, like OpenSSH `ssh-agent`

Go ahead and [submit an issue](https://github.com/overhacked/ssh-agent-mux/issues/new) if there's something that would make `ssh-agent-mux` more useful to you or if it isn't working as it should!

## Installation

### From crates.io

`ssh-agent-mux` can be installed from [crates.io](https://crates.io/crates/ssh-agent-mux):

```console
$ cargo install ssh-agent-mux
```

The minimum supported Rust version is `1.75.0`.

### Binary releases

Download binaries for various operating systems and architectures from the [releases page](https://github.com/overhacked/ssh-agent-mux/releases).

### Build from source

1. Clone the repository:

   ```console
   $ git clone https://github.com/overhacked/ssh-agent-mux.git && cd ssh-agent-mux/
   ```

1. Build:

   ```console
   $ cargo build --release
   ```

   The resulting binary is located at `target/release/ssh-agent-mux`

1. (Optional) Copy the binary to another location on your machine:

   ```console
   $ mkdir -p ~/bin && cp target/release/ssh-agent-mux ~/bin/
   ```

### Home Manager (Nix)

This flake exposes a [Home Manager](https://nix-community.github.io/home-manager/)
module that renders the config file and runs the mux as a user service (systemd
`--user` on Linux, a launchd agent on macOS).

Add the flake as an input and import the module:

```nix
{
  inputs.ssh-agent-mux.url = "github:overhacked/ssh-agent-mux";

  # In your Home Manager configuration:
  imports = [ inputs.ssh-agent-mux.homeManagerModules.default ];

  services.ssh-agent-mux = {
    enable = true;
    # Export SSH_AUTH_SOCK so SSH clients use the mux by default.
    enableSshAuthSock = true;

    agents = [
      {
        name = "1password";
        socketPath = "~/.1password/agent.sock";
      }
      {
        name = "yubikey";
        socketPath = "~/.ssh/yubikey-agent.sock";
      }
    ];

    # Forward `ssh-add` requests to a specific agent (optional).
    addNewKeysTo = "1password";
  };
}
```

Available options: `enable`, `package`, `listenPath`, `agents` (`name`,
`socketPath`, `enabled`), `addNewKeysTo`, `logLevel`, `logFile`, `agentTimeout`,
`installService`, `enableSshAuthSock`, and a freeform `settings` escape hatch.
Because the module manages the service declaratively, the imperative
`ssh-agent-mux service install` / `config install` subcommands are not needed.

## Usage

### Linux (systemd)

```console
$ ssh-agent-mux service install

$ ssh-agent-mux service restart
OR
$ systemctl --user enable --now ssh-agent-mux.service
```

### macOS

```console
$ ssh-agent-mux service install
```

Service will automatically start as soon as it is installed.

### Checking health

`ssh-agent-mux health` diagnoses a running installation end-to-end and prints
the result as [TAP version 14](https://testanything.org/tap-version-14-specification.html).
It is built for the "the service is up but `ssh` can't see my keys" class of
problem: every check is a TAP test point, and failures carry diagnostics
naming the culprit.

```console
$ ssh-agent-mux health
TAP version 14
1..7
ok 1 - config valid
  ---
  path: "/home/you/.config/ssh-agent-mux/ssh-agent-mux.toml"
  agents: 2
  ...
ok 2 - service installed
  ---
  unit: "/home/you/.config/systemd/user/ssh-agent-mux.service"
  ...
ok 3 - service active
  ---
  main-pid: 16891
  ...
ok 4 - listen socket held by service
  ---
  main-pid: 16891
  ...
ok 5 - listen socket answers
  ---
  keys: 3
  ...
ok 6 - upstream 1password answers
  ---
  keys: 2
  ...
ok 7 - upstream yubikey answers
  ---
  keys: 1
  ...
```

The checks, in order:

1. `config valid` --- the configuration parses and validates. On failure the
   run emits `not ok 1 - config valid` with the parse error, then `Bail out!`
   (nothing else is checkable without a config) and exits 1.

1. `service installed` --- the systemd unit (Linux) or launchd plist (macOS)
   is present.

1. `service active` --- the service manager reports the service running.

1. `listen socket held by service` --- the configured listen socket is bound
   by the service's own process rather than a foreign one (Linux only, via
   `/proc`; skipped on macOS). When some other process holds the socket ---
   the classic cause of "service is green but `ssh` sees no keys" --- the
   failure names it:

   ```
   not ok 4 - listen socket held by service
     ---
     holder-pid: 4242
     holder-cgroup: "0::/user.slice/some-other-agent.service"
     ...
   ```

1. `listen socket answers` --- the mux's own socket answers an SSH agent
   protocol request.

1. `upstream <name> answers` --- one check per configured agent, in
   configuration order; agents with `enabled = false` are skipped.

Key counts reported by the protocol checks are diagnostics only: an answering
agent with zero keys still passes. Each protocol probe is bounded by the
`agent-timeout` configuration setting (seconds, default 5).

Checks that cannot run are reported as honest TAP skips, never failures: a
not-installed service skips the dependent service checks, an unavailable
service manager skips as `# SKIP systemctl unavailable`, and so on. Skips do
not affect the exit code.

`--format` selects the output: `tap` (TAP version 14 text, colored on a
terminal), `ndjson` (newline-delimited JSON records, one per check plus a
trailing summary, for machine consumption), or `auto` (the default: TAP when
stdout is a terminal, ndjson when it is piped).

The exit code is `0` when no check failed (skips are fine) and `1` when any
check failed, including the bail-out on an unusable configuration --- so
`ssh-agent-mux health` works directly as a scripted probe.

## Configuration

`ssh-agent-mux` configuration is in [TOML](https://toml.io/en/v1.0.0) format. The default configuration file location is `~/.config/ssh-agent-mux/ssh-agent-mux.toml`. A simple configuration might look like:

```toml
agent_sock_paths = [
	"~/Library/Group Containers/2BUA8C4S2C.com.1password/t/agent.sock",
	"~/Library/Containers/com.maxgoedjen.Secretive.SecretAgent/Data/socket.ssh",
	"~/.ssh/yubikey-agent.sock",
]
```

The order of `agent_sock_paths` affects the order in which public keys are offered to an SSH server. If keys from multiple agents are listed on the server in your `authorized_keys` file, the agent listed first will be the one selected to authenticate with the server.

You can also specify all configuration on the command line, without using a configuration file at all. Any options specified on the command line override configuration file settings. To see the format of command line options, run:

```console
$ ssh-agent-mux --help
```

### Configuration file options

#### `agent_sock_paths` *[Array](https://toml.io/en/v1.0.0#array)*

Socket paths of upstream SSH agents to combine keys from. Must be specified as absolute paths. The order of `agent_sock_paths` affects the order in which public keys are offered to an SSH server. If keys from multiple agents are listed on the server in your `authorized_keys` file, the agent listed first will be the one selected to authenticate with the server.

#### `listen_path` *[String](https://toml.io/en/v1.0.0#string)*

`ssh-agent-mux`'s own socket path. Your SSH client's agent socket (usually the `SSH_AUTH_SOCK` environment variable or the `IdentityAgent` configuration setting) must be set to this path.

*Default*: `~/.ssh/ssh-agent-mux.sock`

#### `log_level` *[String](https://toml.io/en/v1.0.0#string)*

Controls the verbosity of `ssh-agent-mux`'s output. Valid values are: `error`, `warn`, `info`, and `debug`. For development and debugging, the [`RUST_LOG` environment variable](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) is also supported and overrides any `log_level` setting.

*Default*: `warn`

#### `added_keys` *[String](https://toml.io/en/v1.0.0#string)* (Optional)

Socket path of an upstream SSH agent to forward `add_identity` requests to. When SSH keys are added via `ssh-add` to the `ssh-agent-mux` socket, they will be forwarded to this agent. This allows you to add keys to a specific agent through the mux.

*Default*: None (add_identity requests will fail if not configured)

## Related projects

- [`ssh-manager`](https://github.com/omegion/ssh-manager): key manager for 1Password, Bitwarden, and AWS S3
- [`OmniSSHAgent`](https://github.com/masahide/OmniSSHAgent?tab=readme-ov-file): unifies multiple communication methods for SSH agents on Windows
- [`ssh-ident`](https://github.com/ccontavalli/ssh-ident): load ssh-agent identities on demand
- [`sshecret`](https://github.com/thcipriani/sshecret): "wrapper around ssh that automatically manages multiple `ssh-agent`s, each containing only a single ssh key"
- [`sshield`](https://github.com/gotlougit/sshield): drop-in ssh-agent replacement written in Rust using `russh`

## License

Dual-licensed under either [Apache License Version 2.0](https://opensource.org/license/apache-2-0) or [BSD 3-clause License](https://opensource.org/license/bsd-3-clause). You can choose between either one of them if you use this work.

`SPDX-License-Identifier: Apache-2.0 OR BSD-3-Clause`

## Copyright

Copyright © 2024-2025, [Ross Williams](mailto:ross@ross-williams.net)
