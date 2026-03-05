---
status: proposed
date: 2026-03-05
promotion-criteria: step ssh proxycommand succeeds through ssh-agent-mux without "error adding key to agent"
---

# Forward constrained add-identity requests

## Problem Statement

SSH certificate authorities like Smallstep add keys to the agent using `SSH_AGENTC_ADD_ID_CONSTRAINED` (message type 25), which includes lifetime and other constraints on the added key. ssh-agent-mux only implements `add_identity` (message type 17, unconstrained), so constrained add requests fall through to the default `Session` trait implementation which returns `UnsupportedCommand`. This causes `step ssh proxycommand` to fail with `error adding key to agent: agent: failure`, breaking SSH connections through the mux agent.

## Interface

When `add-new-keys-to` is configured, ssh-agent-mux forwards `SSH_AGENTC_ADD_ID_CONSTRAINED` requests to the designated upstream agent, the same way it already forwards `SSH_AGENTC_ADD_IDENTITY` requests. The constraints (lifetime, confirm, extensions) are preserved and passed through to the upstream agent unchanged.

When `add-new-keys-to` is not configured, constrained add requests return `AgentError::Failure`, matching the existing behavior for unconstrained add requests.

The public key from the constrained identity is cached in `known_keys` after a successful forward, the same as unconstrained adds. The `AddIdentityConstrained` type wraps an `AddIdentity` with additional constraint fields, so the credential extraction via `pubkey_from_credential` works without changes.

## Examples

With this configuration:

```toml
listen-path = "~/.local/state/ssh/mux-agent.sock"
add-new-keys-to = "launchd"

[[agents]]
name = "launchd"
socket-path = "~/.local/state/ssh/launchd-agent.sock"
```

Before (fails):

```
$ ssh vm-host
✔ Provisioner: okta (OIDC)
✔ CA: https://ssh.example.ca.smallstep.com
error adding key to agent: agent: failure
Connection closed by UNKNOWN port 65535
```

After (succeeds):

```
$ ssh vm-host
✔ Provisioner: okta (OIDC)
✔ CA: https://ssh.example.ca.smallstep.com
Welcome to vm-host
```

## Limitations

- Constraints are forwarded opaquely to the upstream agent. ssh-agent-mux does not interpret, enforce, or validate constraints itself — it relies on the upstream agent to handle them.
- If the upstream agent does not support constrained adds, the upstream agent's error propagates back to the caller.
