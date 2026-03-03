# Design: `--edit-config` command

## Summary

Add `--edit-config` flag to ssh-agent-mux that opens the config file in the
user's editor, validates the result, atomically replaces the original (honoring
symlinks), and restarts the service.

## Flow

1. Resolve config path from `--config` flag or `XDG_CONFIG_HOME` default.
2. Canonicalize the path (resolve symlinks) to find the real target file.
3. Copy to a temp file in the canonical target's parent directory (same
   filesystem guarantees atomic rename).
4. Open `VISUAL` or `EDITOR` on the temp file.
5. Compare temp file to original. If identical, print "No changes made" and
   exit.
6. Validate via `Config::validate_file()`. On failure, print the error and the
   temp file path, then exit non-zero.
7. `std::fs::rename()` the temp file over the canonical target (atomic).
8. Restart the service (reuse existing restart logic). If the service is not
   installed, skip restart and print a note.

## Editor selection

`VISUAL` first, then `EDITOR`. If neither is set, exit with an error asking the
user to set one. No hardcoded fallback.

## Symlink handling

`std::fs::canonicalize()` resolves the config path before creating the temp file.
The rename targets the canonical path, so symlinks are preserved (only the target
file's contents change).

## Error cases

| Scenario | Behavior |
|---|---|
| Config file missing | Error: run `--install-config` first |
| No VISUAL/EDITOR | Error: set one of them |
| Editor exits non-zero | Error with exit code |
| Validation fails | Print error + temp path, exit non-zero |
| No changes | Print message, exit cleanly |
| Service not installed | Replace config, skip restart, print note |

## Files modified

- `src/bin/ssh-agent-mux/service.rs` -- add `edit_config` to `ServiceArgs`,
  implement handler, wire into `handle_service_command`
