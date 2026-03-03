# `--edit-config` Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to
> implement this plan task-by-task.

**Goal:** Add `--edit-config` flag that edits the config in `VISUAL`/`EDITOR`,
validates, atomically replaces (honoring symlinks), and restarts the service.

**Architecture:** New flag in the existing `ServiceArgs` mutually-exclusive clap
group. A single `handle_edit_config()` function in `service.rs` handles the
full flow: copy to temp, spawn editor, diff, validate, rename, restart. The
`tempfile` crate (already a dev-dependency) is promoted to a regular dependency
for creating temp files in the target directory.

**Tech Stack:** Rust, clap (via clap-serde-derive), tempfile, std::process::Command,
std::fs (canonicalize, rename, read)

---

### Task 1: Move `tempfile` from dev-dependencies to dependencies

**Files:**
- Modify: `Cargo.toml:42-44`

**Step 1: Move the dependency**

In `Cargo.toml`, remove `tempfile = "3.20.0"` from `[dev-dependencies]` and add
it to `[dependencies]`:

```toml
[dependencies]
# ... existing deps ...
tempfile = "3.20.0"
```

`[dev-dependencies]` should only have `duct` remaining.

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: success

**Step 3: Commit**

```
feat: move tempfile to regular dependency for edit-config
```

---

### Task 2: Add `edit_config` flag to `ServiceArgs`

**Files:**
- Modify: `src/bin/ssh-agent-mux/service.rs:17-50`

**Step 1: Add the flag**

Add to the `ServiceArgs` struct after `validate_config`:

```rust
    /// Edit the configuration file in $VISUAL or $EDITOR
    #[arg(long)]
    pub edit_config: bool,
```

**Step 2: Add it to the `any()` method**

Update the `any()` method to include `self.edit_config`:

```rust
    pub fn any(&self) -> bool {
        self.install_service
            || self.restart_service
            || self.uninstall_service
            || self.install_config
            || self.validate_config
            || self.edit_config
    }
```

**Step 3: Verify it compiles**

Run: `cargo check`
Expected: success

**Step 4: Commit**

```
feat: add --edit-config flag to ServiceArgs
```

---

### Task 3: Implement `handle_edit_config()`

**Files:**
- Modify: `src/bin/ssh-agent-mux/service.rs`

**Step 1: Add imports**

Add `std::process::Command` to the existing `use std::` import at line 1:

```rust
use std::{env, ffi::OsString, fmt::Write, fs, io, path::PathBuf, process::Command};
```

**Step 2: Write the `resolve_editor()` helper**

Add after `handle_set_level_error()` (end of file):

```rust
fn resolve_editor() -> Result<String> {
    env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .map_err(|_| eyre!("Set VISUAL or EDITOR environment variable to use --edit-config"))
}
```

**Step 3: Write `handle_edit_config()`**

Add after `resolve_editor()`:

```rust
fn handle_edit_config(config: &Config) -> Result<()> {
    if !config.config_path.try_exists()? {
        bail!(
            "No config file found at {}; run --install-config first",
            config.config_path.display()
        );
    }

    let editor = resolve_editor()?;

    // Resolve symlinks so we replace the target, not the symlink
    let canonical_path = fs::canonicalize(&config.config_path)
        .wrap_err_with(|| format!("Failed to resolve {}", config.config_path.display()))?;

    let canonical_dir = canonical_path
        .parent()
        .ok_or_else(|| eyre!("Config path has no parent directory"))?;

    let original_contents = fs::read(&canonical_path)?;

    // Create temp file in same directory as target for atomic rename
    let temp_file = tempfile::Builder::new()
        .prefix("ssh-agent-mux-")
        .suffix(".toml")
        .tempfile_in(canonical_dir)
        .wrap_err("Failed to create temporary file")?;

    let temp_path = temp_file.into_temp_path();
    fs::write(&temp_path, &original_contents)?;

    let status = Command::new(&editor)
        .arg(&temp_path)
        .status()
        .wrap_err_with(|| format!("Failed to launch editor: {}", editor))?;

    if !status.success() {
        let code = status.code().map_or("unknown".to_string(), |c| c.to_string());
        // Clean up temp file on editor failure
        let _ = fs::remove_file(&temp_path);
        bail!("Editor exited with status {}", code);
    }

    let edited_contents = fs::read(&temp_path)?;

    if original_contents == edited_contents {
        // temp_path is dropped here, which deletes the file
        println!("No changes made");
        return Ok(());
    }

    if let Err(err) = Config::validate_file(&temp_path) {
        let kept_path = temp_path.keep()
            .map_err(|e| eyre!("Failed to persist temp file: {}", e.error))?;
        return Err(err)
            .wrap_err(format!(
                "Validation failed; your edits are saved at {}",
                kept_path.display()
            ));
    }

    // Atomic replace: rename within same filesystem
    fs::rename(&temp_path, &canonical_path)
        .wrap_err("Failed to replace config file")?;

    println!("Updated config at {}", config.config_path.display());

    restart_service_if_running()
}
```

**Step 4: Extract restart logic into `restart_service_if_running()`**

Add before `handle_edit_config()`:

```rust
fn restart_service_if_running() -> Result<()> {
    let manager = match <dyn ServiceManager>::native() {
        Ok(mut m) => {
            if let Err(err) = m.set_level(service_manager::ServiceLevel::User) {
                if err.kind() == io::ErrorKind::Unsupported {
                    println!("Service management not supported on this platform; skipping restart");
                    return Ok(());
                }
                return Err(err.into());
            }
            m
        }
        Err(_) => {
            println!("No service manager available; skipping restart");
            return Ok(());
        }
    };

    let label: service_manager::ServiceLabel =
        SERVICE_IDENT.parse().expect("SERVICE_IDENT is wrong");

    let status = manager.status(ServiceStatusCtx {
        label: label.clone(),
    })?;

    match status {
        ServiceStatus::Running => {
            manager.stop(ServiceStopCtx {
                label: label.clone(),
            })?;
            manager.start(ServiceStartCtx { label })?;
            println!("Restarted service {}", SERVICE_IDENT);
        }
        ServiceStatus::Stopped(_) => {
            manager.start(ServiceStartCtx { label })?;
            println!("Started service {}", SERVICE_IDENT);
        }
        ServiceStatus::NotInstalled => {
            println!("Service not installed; skipping restart");
        }
    }

    Ok(())
}
```

**Step 5: Wire into `handle_service_command()`**

In `handle_service_command()`, add a check for `edit_config` before the
`validate_config` check (line 53). The `edit_config` handler is placed first
because it handles its own service manager interaction internally:

```rust
pub fn handle_service_command(config: &Config) -> Result<()> {
    if config.service.edit_config {
        return handle_edit_config(config);
    }

    if config.service.validate_config {
        // ... existing code
```

**Step 6: Verify it compiles**

Run: `cargo check`
Expected: success

**Step 7: Commit**

```
feat: implement --edit-config command
```

---

### Task 4: Add unit tests for `resolve_editor()`

**Files:**
- Modify: `src/bin/ssh-agent-mux/service.rs` (add `#[cfg(test)]` module)

**Step 1: Write the tests**

Add at the end of `service.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn resolve_editor_prefers_visual_over_editor() {
        env::set_var("VISUAL", "code");
        env::set_var("EDITOR", "vim");
        let result = resolve_editor().unwrap();
        assert_eq!(result, "code");
        env::remove_var("VISUAL");
        env::remove_var("EDITOR");
    }

    #[test]
    fn resolve_editor_falls_back_to_editor() {
        env::remove_var("VISUAL");
        env::set_var("EDITOR", "nano");
        let result = resolve_editor().unwrap();
        assert_eq!(result, "nano");
        env::remove_var("EDITOR");
    }

    #[test]
    fn resolve_editor_fails_when_neither_set() {
        env::remove_var("VISUAL");
        env::remove_var("EDITOR");
        let result = resolve_editor();
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("VISUAL"));
        assert!(msg.contains("EDITOR"));
    }
}
```

**Step 2: Run the tests**

Run: `cargo test --bin ssh-agent-mux -- resolve_editor`
Expected: 3 tests pass

Note: these tests modify environment variables, which is not thread-safe. Cargo
runs tests in the same binary in parallel by default. If flaky, add
`--test-threads=1`. The existing tests in `cli.rs` already do `env::set_var`
without synchronization, so this matches the project convention.

**Step 3: Commit**

```
test: add unit tests for resolve_editor
```

---

### Task 5: Manual verification

**Step 1: Build the binary**

Run: `cargo build`

**Step 2: Test the happy path**

Create a test config, run `--edit-config`, make a change, verify the config was
updated and service restart was attempted.

**Step 3: Test no-change path**

Run `--edit-config`, exit the editor without changes. Verify "No changes made"
output.

**Step 4: Test validation failure**

Run `--edit-config`, introduce an invalid field, save and exit. Verify the error
message includes the temp file path.

**Step 5: Test symlink handling**

Create a symlink to a config file, run `--edit-config` with `--config` pointing
at the symlink. Verify the symlink still points to the same target, and the
target's contents were updated.

**Step 6: Commit with verification note**

```
test: verify --edit-config against real editor and service manager
```
