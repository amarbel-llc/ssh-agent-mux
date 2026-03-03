
- [ ] change install-ssh-agent-mux output to TAP-14 format (currently human-readable "Restarted service ...")
- [ ] stale socket not cleaned up on start: if a socket file exists from a previous run, bind fails with EADDRINUSE instead of removing the stale socket and rebinding
