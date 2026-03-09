//! Integration tests for the `smelt` CLI binary.
//!
//! Each test creates an isolated temporary directory to avoid cross-contamination.

use assert_cmd::Command;
use predicates::prelude::*;

/// Create a temporary directory containing a bare-minimum git repo with one commit.
fn setup_git_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("create temp dir");

    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(tmp.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env("HOME", tmp.path())
            .output()
            .expect("git command should run")
    };

    let out = git(&["init"]);
    assert!(out.status.success(), "git init failed");

    let out = git(&["commit", "--allow-empty", "-m", "initial"]);
    assert!(out.status.success(), "git commit failed");

    tmp
}

/// Build a `Command` for the `smelt` binary, pre-configured with environment
/// isolation and `current_dir` pointing at the given path.
fn smelt_cmd(dir: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("smelt").expect("binary should be built");
    cmd.current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir);
    cmd
}

// ── Version & Help ─────────────────────────────────────────────────────

#[test]
fn test_version() {
    let tmp = setup_git_repo();
    smelt_cmd(tmp.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("smelt"));
}

#[test]
fn test_help() {
    let tmp = setup_git_repo();
    smelt_cmd(tmp.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("init"));
}

// ── Init ───────────────────────────────────────────────────────────────

#[test]
fn test_init_creates_smelt_dir() {
    let tmp = setup_git_repo();

    smelt_cmd(tmp.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized"));

    let config_path = tmp.path().join(".smelt/config.toml");
    assert!(config_path.exists(), ".smelt/config.toml should exist");

    let contents = std::fs::read_to_string(&config_path).expect("read config.toml");
    assert!(
        contents.contains("version = 1"),
        "config.toml should contain version = 1, got: {contents}",
    );
}

#[test]
fn test_init_already_initialized() {
    let tmp = setup_git_repo();

    // First init succeeds
    smelt_cmd(tmp.path()).arg("init").assert().success();

    // Second init fails
    smelt_cmd(tmp.path())
        .arg("init")
        .assert()
        .code(1)
        .stderr(predicate::str::is_match("(?i)already").expect("valid regex"));
}

// ── No-args Behavior ───────────────────────────────────────────────────

#[test]
fn test_no_args_outside_project() {
    let tmp = setup_git_repo();

    smelt_cmd(tmp.path())
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Not a Smelt project"));
}

#[test]
fn test_no_args_inside_project() {
    let tmp = setup_git_repo();

    // Initialize first
    smelt_cmd(tmp.path()).arg("init").assert().success();

    // No-args should show status
    smelt_cmd(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Smelt project"));
}

// ── Flags ──────────────────────────────────────────────────────────────

#[test]
fn test_no_color_flag() {
    let tmp = setup_git_repo();
    smelt_cmd(tmp.path())
        .args(["--no-color", "--help"])
        .assert()
        .success();
}

// ── Error Paths ────────────────────────────────────────────────────────

#[test]
fn test_outside_git_repo() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    smelt_cmd(tmp.path())
        .assert()
        .code(1)
        .stderr(predicate::str::is_match("(?i)git").expect("valid regex"));
}
