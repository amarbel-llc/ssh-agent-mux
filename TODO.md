
- [ ] change install-ssh-agent-mux output to TAP-14 format (currently human-readable "Restarted service ...")
- [ ] stale socket not cleaned up on start: if a socket file exists from a previous run, bind fails with EADDRINUSE instead of removing the stale socket and rebinding
- [ ] --edit-config: split VISUAL/EDITOR on whitespace to support `VISUAL="code --wait"`
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
