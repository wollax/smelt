//! Integration tests for `smelt merge` CLI command.
//!
//! Each test creates a git repo as a subdirectory of the temp dir, so worktrees
//! land as siblings inside the temp dir and are cleaned up automatically.

use assert_cmd::Command;
use predicates::prelude::*;

/// Create a git repo at `tmp/test-repo/` with an initial commit and return the repo path.
fn setup_test_repo(tmp: &tempfile::TempDir) -> std::path::PathBuf {
    let repo_path = tmp.path().join("test-repo");
    std::fs::create_dir(&repo_path).expect("create repo dir");

    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&repo_path)
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

    std::fs::write(repo_path.join("README.md"), "# test\n").unwrap();
    let out = git(&["add", "README.md"]);
    assert!(out.status.success(), "git add failed");
    let out = git(&["commit", "-m", "initial"]);
    assert!(out.status.success(), "git commit failed");

    repo_path
}

/// Write a manifest TOML file in the repo and return its path.
fn write_manifest(repo_path: &std::path::Path, content: &str) -> std::path::PathBuf {
    let manifest_path = repo_path.join("manifest.toml");
    std::fs::write(&manifest_path, content).expect("write manifest");
    manifest_path
}

/// Build a `Command` for the `smelt` binary with environment isolation.
fn smelt_cmd(dir: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("smelt").expect("binary should be built");
    cmd.current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("HOME", dir);
    cmd
}

/// Helper to run a git command in a directory and return its output.
fn git_in(dir: &std::path::Path, tmp_home: &std::path::Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .env("HOME", tmp_home)
        .output()
        .expect("git command should run")
}

// ── Test 1: Clean merge of two sessions ─────────────────────────────

#[test]
fn test_merge_clean_two_sessions() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "two-clean"

[[session]]
name = "session-alpha"
task = "Write alpha file"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add alpha.txt"
files = [{ path = "alpha.txt", content = "alpha content\n" }]

[[session]]
name = "session-beta"
task = "Write beta file"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add beta.txt"
files = [{ path = "beta.txt", content = "beta content\n" }]
"#,
    );

    // Run sessions first
    smelt_cmd(&repo)
        .args(["session", "run", manifest.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("2/2 sessions completed"));

    // Now merge
    smelt_cmd(&repo)
        .args(["merge", "run", manifest.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("session-alpha:"))
        .stdout(predicate::str::contains("session-beta:"))
        .stdout(predicate::str::contains("file(s) changed"))
        .stderr(predicate::str::contains("Merged 2 session(s)"));

    // Verify target branch exists
    let output = git_in(&repo, tmp.path(), &["branch", "--list", "smelt/merge/*"]);
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(
        branches.contains("smelt/merge/two-clean"),
        "target branch should exist, got: {branches}"
    );
}

// ── Test 2: Merge conflict exits with error ─────────────────────────

#[test]
fn test_merge_conflict_exits_with_error() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "conflict-merge"

[[session]]
name = "session-a"
task = "Edit shared file version A"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "Session A changes"
files = [{ path = "shared.txt", content = "content from A\n" }]

[[session]]
name = "session-b"
task = "Edit shared file version B"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "Session B changes"
files = [{ path = "shared.txt", content = "content from B\n" }]
"#,
    );

    // Run sessions
    smelt_cmd(&repo)
        .args(["session", "run", manifest.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("2/2 sessions completed"));

    // Merge should fail with conflict
    smelt_cmd(&repo)
        .args(["merge", "run", manifest.to_str().unwrap()])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("merge conflict"))
        .stderr(predicate::str::contains("shared.txt"));

    // Target branch should NOT exist (rolled back)
    let output = git_in(&repo, tmp.path(), &["branch", "--list", "smelt/merge/conflict-merge"]);
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(
        branches.trim().is_empty(),
        "target branch should not exist after conflict, got: {branches}"
    );
}

// ── Test 3: Merge with custom target branch ─────────────────────────

#[test]
fn test_merge_with_custom_target() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "custom-target"

[[session]]
name = "session-one"
task = "Write one file"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add one.txt"
files = [{ path = "one.txt", content = "one\n" }]
"#,
    );

    // Run sessions
    smelt_cmd(&repo)
        .args(["session", "run", manifest.to_str().unwrap()])
        .assert()
        .success();

    // Merge with custom target
    smelt_cmd(&repo)
        .args(["merge", "run", manifest.to_str().unwrap(), "--target", "my-custom-branch"])
        .assert()
        .success()
        .stderr(predicate::str::contains("my-custom-branch"));

    // Verify custom branch exists
    let output = git_in(&repo, tmp.path(), &["branch", "--list", "my-custom-branch"]);
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(
        branches.contains("my-custom-branch"),
        "custom target branch should exist, got: {branches}"
    );

    // Verify default branch does NOT exist
    let output = git_in(&repo, tmp.path(), &["branch", "--list", "smelt/merge/custom-target"]);
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(
        branches.trim().is_empty(),
        "default branch should not exist, got: {branches}"
    );
}

// ── Test 4: Target branch already exists ────────────────────────────

#[test]
fn test_merge_target_exists_error() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "exists-test"

[[session]]
name = "session-one"
task = "Write one file"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add one.txt"
files = [{ path = "one.txt", content = "one\n" }]
"#,
    );

    // Run sessions
    smelt_cmd(&repo)
        .args(["session", "run", manifest.to_str().unwrap()])
        .assert()
        .success();

    // Pre-create the target branch
    let out = git_in(&repo, tmp.path(), &["branch", "smelt/merge/exists-test"]);
    assert!(out.status.success(), "should create branch");

    // Merge should fail because target exists
    smelt_cmd(&repo)
        .args(["merge", "run", manifest.to_str().unwrap()])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("already exists"));
}

// ── Test 5: No sessions run (no state files) ────────────────────────

#[test]
fn test_merge_no_sessions_run() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "no-sessions"

[[session]]
name = "session-one"
task = "Write one file"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add one.txt"
files = [{ path = "one.txt", content = "one\n" }]
"#,
    );

    // Don't run sessions — go straight to merge
    smelt_cmd(&repo)
        .args(["merge", "run", manifest.to_str().unwrap()])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("no completed sessions"));
}

// ── Test 6: Manifest file not found ─────────────────────────────────

#[test]
fn test_merge_manifest_not_found() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    smelt_cmd(&repo)
        .args(["merge", "run", "nonexistent.toml"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Error"));
}

// ── Test 7: Three sessions, one failed — merge skips failed ─────────

#[test]
fn test_merge_three_sessions_one_failed() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "mixed-results"

[[session]]
name = "good-one"
task = "Write file one"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add one.txt"
files = [{ path = "one.txt", content = "one\n" }]

[[session]]
name = "bad-one"
task = "This will crash"

[session.script]
backend = "scripted"
exit_after = 1
simulate_failure = "crash"

[[session.script.steps]]
action = "commit"
message = "add bad.txt"
files = [{ path = "bad.txt", content = "bad\n" }]

[[session.script.steps]]
action = "commit"
message = "second"
files = [{ path = "bad2.txt", content = "bad2\n" }]

[[session]]
name = "good-two"
task = "Write file two"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add two.txt"
files = [{ path = "two.txt", content = "two\n" }]
"#,
    );

    // Run sessions — one will fail
    smelt_cmd(&repo)
        .args(["session", "run", manifest.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("2/3 sessions completed"));

    // Merge should succeed with 2 sessions, skip the failed one
    smelt_cmd(&repo)
        .args(["merge", "run", manifest.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("good-one:"))
        .stdout(predicate::str::contains("good-two:"))
        .stderr(predicate::str::contains("Merged 2 session(s)"))
        .stderr(predicate::str::contains("Skipped 1 session(s)"))
        .stderr(predicate::str::contains("bad-one"));
}
