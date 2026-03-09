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

/// Clean up sibling worktree directories created by `smelt worktree create`.
///
/// Worktrees are placed at `{tmp_parent}/{tmp_dir_name}-smelt-{session_name}/`.
fn cleanup_worktree_sibling(tmp: &tempfile::TempDir, session_name: &str) {
    let tmp_dir_name = tmp
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let parent = tmp.path().parent().unwrap();
    let sibling = parent.join(format!("{tmp_dir_name}-smelt-{session_name}"));
    let _ = std::fs::remove_dir_all(&sibling);
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

// ── Worktree Lifecycle ─────────────────────────────────────────────────

#[test]
fn test_worktree_create_and_list() {
    let tmp = setup_git_repo();

    // Init
    smelt_cmd(tmp.path()).arg("init").assert().success();

    // Create worktree
    smelt_cmd(tmp.path())
        .args(["worktree", "create", "test-session"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created worktree 'test-session'"))
        .stdout(predicate::str::contains("smelt/test-session"));

    // List should show the worktree
    smelt_cmd(tmp.path())
        .args(["worktree", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-session"))
        .stdout(predicate::str::contains("smelt/test-session"));

    // Cleanup
    cleanup_worktree_sibling(&tmp, "test-session");
}

#[test]
fn test_worktree_create_duplicate() {
    let tmp = setup_git_repo();

    smelt_cmd(tmp.path()).arg("init").assert().success();

    // First create succeeds
    smelt_cmd(tmp.path())
        .args(["worktree", "create", "dup-test"])
        .assert()
        .success();

    // Second create fails
    smelt_cmd(tmp.path())
        .args(["worktree", "create", "dup-test"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("already exists"));

    // Cleanup
    cleanup_worktree_sibling(&tmp, "dup-test");
}

#[test]
fn test_worktree_remove() {
    let tmp = setup_git_repo();

    smelt_cmd(tmp.path()).arg("init").assert().success();

    // Create
    smelt_cmd(tmp.path())
        .args(["worktree", "create", "remove-test"])
        .assert()
        .success();

    // Remove with --yes and --force (branch won't be merged since it's just created)
    smelt_cmd(tmp.path())
        .args(["worktree", "remove", "remove-test", "--yes", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed worktree 'remove-test'"));

    // List should be empty
    smelt_cmd(tmp.path())
        .args(["worktree", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No worktrees tracked."));

    // Cleanup (should already be gone, but just in case)
    cleanup_worktree_sibling(&tmp, "remove-test");
}

#[test]
fn test_worktree_wt_alias() {
    let tmp = setup_git_repo();

    smelt_cmd(tmp.path()).arg("init").assert().success();

    // `smelt wt list` should work same as `smelt worktree list`
    smelt_cmd(tmp.path())
        .args(["wt", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No worktrees tracked."));

    // Create via alias
    smelt_cmd(tmp.path())
        .args(["wt", "create", "alias-test"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created worktree 'alias-test'"));

    // List via alias
    smelt_cmd(tmp.path())
        .args(["wt", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alias-test"));

    // Cleanup
    cleanup_worktree_sibling(&tmp, "alias-test");
}

#[test]
fn test_worktree_create_without_init() {
    let tmp = setup_git_repo();

    // Attempt create without init
    smelt_cmd(tmp.path())
        .args(["worktree", "create", "no-init"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("smelt init"));
}

#[test]
fn test_worktree_remove_nonexistent() {
    let tmp = setup_git_repo();

    smelt_cmd(tmp.path()).arg("init").assert().success();

    // Remove a name that doesn't exist
    smelt_cmd(tmp.path())
        .args(["worktree", "remove", "ghost", "--yes"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_worktree_remove_dirty_with_force() {
    let tmp = setup_git_repo();
    smelt_cmd(tmp.path()).arg("init").assert().success();

    // Create worktree
    smelt_cmd(tmp.path())
        .args(["worktree", "create", "dirty-test"])
        .assert()
        .success();

    // Make the worktree dirty by writing an untracked file
    let tmp_dir_name = tmp.path().file_name().unwrap().to_string_lossy().to_string();
    let wt_dir = tmp
        .path()
        .parent()
        .unwrap()
        .join(format!("{tmp_dir_name}-smelt-dirty-test"));
    std::fs::write(wt_dir.join("dirty-file.txt"), "dirty\n").expect("write dirty file");

    // Remove with --force --yes should succeed even with dirty worktree
    smelt_cmd(tmp.path())
        .args(["worktree", "remove", "dirty-test", "--force", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed worktree 'dirty-test'"));

    // Cleanup
    cleanup_worktree_sibling(&tmp, "dirty-test");
}

#[test]
fn test_worktree_remove_dirty_with_yes_auto_confirms() {
    let tmp = setup_git_repo();
    smelt_cmd(tmp.path()).arg("init").assert().success();

    // Create worktree
    smelt_cmd(tmp.path())
        .args(["worktree", "create", "dirty-yes-test"])
        .assert()
        .success();

    // Make the worktree dirty
    let tmp_dir_name = tmp.path().file_name().unwrap().to_string_lossy().to_string();
    let wt_dir = tmp
        .path()
        .parent()
        .unwrap()
        .join(format!("{tmp_dir_name}-smelt-dirty-yes-test"));
    std::fs::write(wt_dir.join("dirty-file.txt"), "dirty\n").expect("write dirty file");

    // Remove with --yes (no --force) should auto-confirm and force remove
    smelt_cmd(tmp.path())
        .args(["worktree", "remove", "dirty-yes-test", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed worktree 'dirty-yes-test'"));

    // Cleanup
    cleanup_worktree_sibling(&tmp, "dirty-yes-test");
}
