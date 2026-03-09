
- [ ] change install-ssh-agent-mux output to TAP-14 format (currently human-readable "Restarted service ...")
- [ ] stale socket not cleaned up on start: if a socket file exists from a previous run, bind fails with EADDRINUSE instead of removing the stale socket and rebinding
- [ ] --edit-config: split VISUAL/EDITOR on whitespace to support `VISUAL="code --wait"`
- [ ] ssh-agent-lib 0.5.1 cannot round-trip certificate identities (ECDSA-CERT etc): `KeyData::decode` treats cert algorithm as `Algorithm::Other`/`OpaquePublicKey`, truncates the cert blob on decode, then re-serializes a malformed response causing "incomplete message" errors in `ssh-add -l`
- [ ] not ok 10 - install-ssh-agent-mux
  ---
  message: "error: Recipe `install-ssh-agent-mux` failed with exit code 1"
  severity: fail
  exitcode: 1
  output: |
    Error: 
       0: Command failed with exit code 3: Failed to execute command with no output
    
    Location:
       src/bin/ssh-agent-mux/service.rs:116
  ...
