use std::{
    fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
};

use duct::cmd;
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn make_editor_script(body: &str) -> io::Result<tempfile::TempPath> {
    let mut script = tempfile::Builder::new()
        .prefix("editor_")
        .suffix(".sh")
        .tempfile_in(std::env::temp_dir())?;
    write!(script, "#!/bin/sh\n{body}")?;
    let path = script.into_temp_path();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    Ok(path)
}

fn write_valid_config(dir: &Path) -> io::Result<()> {
    let config_dir = dir.join("ssh-agent-mux");
    fs::create_dir_all(&config_dir)?;
    fs::write(
        config_dir.join("ssh-agent-mux.toml"),
        r#"listen-path = "/tmp/test-mux.sock"
log-level = "warn"

[[agents]]
name = "default"
socket-path = "/tmp/test-agent.sock"
"#,
    )
}

fn config_path(dir: &Path) -> String {
    dir.join("ssh-agent-mux/ssh-agent-mux.toml")
        .display()
        .to_string()
}

fn run_edit_config(
    config_dir: &Path,
    editor: &Path,
) -> Result<std::process::Output, duct::Expression> {
    let output = cmd!(
        env!("CARGO_BIN_EXE_ssh-agent-mux"),
        "--config",
        config_path(config_dir),
        "--edit-config"
    )
    .env("EDITOR", editor)
    .env_remove("VISUAL")
    .unchecked()
    .stdout_capture()
    .stderr_capture()
    .run();

    output.map_err(|_| {
        cmd!(
            env!("CARGO_BIN_EXE_ssh-agent-mux"),
            "--config",
            config_path(config_dir),
            "--edit-config"
        )
    })
}

#[test]
fn edit_config_no_changes() -> TestResult {
    let dir = TempDir::new()?;
    write_valid_config(dir.path())?;

    // Editor that does nothing (file unchanged)
    let editor = make_editor_script("exit 0")?;

    let output = run_edit_config(dir.path(), &editor).unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "expected success, stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains("No changes made"), "expected 'No changes made', got: {stdout}");

    Ok(())
}

#[test]
fn edit_config_valid_change() -> TestResult {
    let dir = TempDir::new()?;
    write_valid_config(dir.path())?;

    let original = fs::read_to_string(config_path(dir.path()))?;

    // Editor that changes the log level
    let editor =
        make_editor_script(r#"sed -i.bak 's/log-level = "warn"/log-level = "info"/' "$1""#)?;

    let output = run_edit_config(dir.path(), &editor).unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success(), "expected success, stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains("Updated config"), "expected 'Updated config', got: {stdout}");

    let updated = fs::read_to_string(config_path(dir.path()))?;
    assert_ne!(original, updated);
    assert!(updated.contains(r#"log-level = "info""#));

    Ok(())
}

#[test]
fn edit_config_invalid_change_preserves_original() -> TestResult {
    let dir = TempDir::new()?;
    write_valid_config(dir.path())?;

    let original = fs::read_to_string(config_path(dir.path()))?;

    // Editor that adds an unknown field (rejected by deny_unknown_fields)
    let editor = make_editor_script(
        r#"printf '\nbogus = true\n' >> "$1""#,
    )?;

    let output = run_edit_config(dir.path(), &editor).unwrap();

    assert!(!output.status.success(), "expected failure");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Validation failed"),
        "expected validation error, got: {stderr}"
    );
    assert!(
        stderr.contains("your edits are saved at"),
        "expected temp file path in error, got: {stderr}"
    );

    // Original config is untouched
    let after = fs::read_to_string(config_path(dir.path()))?;
    assert_eq!(original, after);

    Ok(())
}

#[test]
fn edit_config_editor_failure() -> TestResult {
    let dir = TempDir::new()?;
    write_valid_config(dir.path())?;

    let original = fs::read_to_string(config_path(dir.path()))?;

    // Editor that exits with error
    let editor = make_editor_script("exit 1")?;

    let output = run_edit_config(dir.path(), &editor).unwrap();

    assert!(!output.status.success(), "expected failure");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Editor exited with status"),
        "expected editor error, got: {stderr}"
    );

    // Original config is untouched
    let after = fs::read_to_string(config_path(dir.path()))?;
    assert_eq!(original, after);

    Ok(())
}

#[test]
fn edit_config_missing_config() -> TestResult {
    let dir = TempDir::new()?;
    // Don't create any config file

    let editor = make_editor_script("exit 0")?;

    let output = cmd!(
        env!("CARGO_BIN_EXE_ssh-agent-mux"),
        "--config",
        dir.path().join("nonexistent.toml").display().to_string(),
        "--edit-config"
    )
    .env("EDITOR", AsRef::<Path>::as_ref(&editor))
    .env_remove("VISUAL")
    .unchecked()
    .stdout_capture()
    .stderr_capture()
    .run()?;

    assert!(!output.status.success(), "expected failure");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No config file found") || stderr.contains("install-config"),
        "expected missing config error, got: {stderr}"
    );

    Ok(())
}

#[test]
fn edit_config_preserves_symlink() -> TestResult {
    let dir = TempDir::new()?;
    let target_dir = dir.path().join("dotfiles");
    let link_dir = dir.path().join("config/ssh-agent-mux");

    fs::create_dir_all(&target_dir)?;
    fs::create_dir_all(&link_dir)?;

    let target_file = target_dir.join("ssh-agent-mux.toml");
    fs::write(
        &target_file,
        r#"listen-path = "/tmp/test-mux.sock"
log-level = "warn"

[[agents]]
name = "default"
socket-path = "/tmp/test-agent.sock"
"#,
    )?;

    let link_file = link_dir.join("ssh-agent-mux.toml");
    std::os::unix::fs::symlink(&target_file, &link_file)?;

    assert!(link_file.symlink_metadata()?.file_type().is_symlink());

    // Editor that changes log level
    let editor =
        make_editor_script(r#"sed -i.bak 's/log-level = "warn"/log-level = "info"/' "$1""#)?;

    let output = cmd!(
        env!("CARGO_BIN_EXE_ssh-agent-mux"),
        "--config",
        link_file.display().to_string(),
        "--edit-config"
    )
    .env("EDITOR", AsRef::<Path>::as_ref(&editor))
    .env_remove("VISUAL")
    .unchecked()
    .stdout_capture()
    .stderr_capture()
    .run()?;

    assert!(output.status.success(), "expected success, stderr: {}", String::from_utf8_lossy(&output.stderr));

    // Symlink is preserved
    assert!(
        link_file.symlink_metadata()?.file_type().is_symlink(),
        "symlink was replaced with a regular file"
    );

    // Content was updated (readable through symlink)
    let updated = fs::read_to_string(&link_file)?;
    assert!(updated.contains(r#"log-level = "info""#));

    // Target file was also updated
    let target_content = fs::read_to_string(&target_file)?;
    assert!(updated.contains(r#"log-level = "info""#));
    assert_eq!(updated, target_content);

    Ok(())
}

#[test]
fn edit_config_no_editor_set() -> TestResult {
    let dir = TempDir::new()?;
    write_valid_config(dir.path())?;

    let output = cmd!(
        env!("CARGO_BIN_EXE_ssh-agent-mux"),
        "--config",
        config_path(dir.path()),
        "--edit-config"
    )
    .env_remove("VISUAL")
    .env_remove("EDITOR")
    .unchecked()
    .stdout_capture()
    .stderr_capture()
    .run()?;

    assert!(!output.status.success(), "expected failure");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("VISUAL") && stderr.contains("EDITOR"),
        "expected editor env var error, got: {stderr}"
    );

    Ok(())
}
