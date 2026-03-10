//! Integration tests for `smelt orchestrate run` CLI command.
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
        .env("HOME", dir)
        .env("NO_COLOR", "1");
    cmd
}

/// Helper to run a git command in a directory and return its output.
fn git_in(
    dir: &std::path::Path,
    tmp_home: &std::path::Path,
    args: &[&str],
) -> std::process::Output {
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

// ── Test 1: Two parallel sessions ───────────────────────────────────

#[test]
fn orchestrate_two_parallel_sessions() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "two-parallel"

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

    let assert = smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("session-alpha"),
        "stdout should contain session-alpha, got: {stdout}"
    );
    assert!(
        stdout.contains("session-beta"),
        "stdout should contain session-beta, got: {stdout}"
    );
    assert!(
        stdout.contains("done"),
        "stdout should show done status, got: {stdout}"
    );

    // Verify merged branch exists with files from both sessions
    let output = git_in(&repo, tmp.path(), &["branch", "--list", "smelt/merge/*"]);
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(
        branches.contains("smelt/merge/two-parallel"),
        "merged branch should exist, got: {branches}"
    );

    // Check files exist on merged branch
    let output = git_in(
        &repo,
        tmp.path(),
        &["ls-tree", "--name-only", "smelt/merge/two-parallel"],
    );
    let files = String::from_utf8_lossy(&output.stdout);
    assert!(
        files.contains("alpha.txt"),
        "merged branch should have alpha.txt, got: {files}"
    );
    assert!(
        files.contains("beta.txt"),
        "merged branch should have beta.txt, got: {files}"
    );
}

// ── Test 2: Sequential dependency ───────────────────────────────────

#[test]
fn orchestrate_sequential_dependency() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "sequential-dep"

[[session]]
name = "session-a"
task = "Write file A"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add a.txt"
files = [{ path = "a.txt", content = "a content\n" }]

[[session]]
name = "session-b"
task = "Write file B"
depends_on = ["session-a"]

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add b.txt"
files = [{ path = "b.txt", content = "b content\n" }]
"#,
    );

    let assert = smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("session-a") && stdout.contains("session-b"),
        "both sessions should appear in output, got: {stdout}"
    );
    assert!(
        stdout.contains("done"),
        "sessions should be done, got: {stdout}"
    );

    // Verify merged branch exists
    let output = git_in(&repo, tmp.path(), &["branch", "--list", "smelt/merge/*"]);
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(
        branches.contains("smelt/merge/sequential-dep"),
        "merged branch should exist, got: {branches}"
    );
}

// ── Test 3: Skip dependents on failure ──────────────────────────────

#[test]
fn orchestrate_skip_dependents_on_failure() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "skip-deps"
on_failure = "skip-dependents"

[[session]]
name = "session-a"
task = "This will crash"

[session.script]
backend = "scripted"
exit_after = 1
simulate_failure = "crash"

[[session.script.steps]]
action = "commit"
message = "add a.txt"
files = [{ path = "a.txt", content = "a\n" }]

[[session.script.steps]]
action = "commit"
message = "second"
files = [{ path = "a2.txt", content = "a2\n" }]

[[session]]
name = "session-b"
task = "Depends on A"
depends_on = ["session-a"]

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add b.txt"
files = [{ path = "b.txt", content = "b\n" }]
"#,
    );

    let assert = smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .assert()
        .code(1);

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("failed"),
        "should show failed status, got: {stdout}"
    );
    assert!(
        stdout.contains("skipped"),
        "should show skipped status for dependent, got: {stdout}"
    );
}

// ── Test 4: Abort on failure ────────────────────────────────────────

#[test]
fn orchestrate_abort_on_failure() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "abort-test"
on_failure = "abort"

[[session]]
name = "crasher"
task = "Will crash"

[session.script]
backend = "scripted"
exit_after = 1
simulate_failure = "crash"

[[session.script.steps]]
action = "commit"
message = "first"
files = [{ path = "a.txt", content = "a\n" }]

[[session.script.steps]]
action = "commit"
message = "second"
files = [{ path = "b.txt", content = "b\n" }]

[[session]]
name = "independent"
task = "Independent session"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add c.txt"
files = [{ path = "c.txt", content = "c\n" }]
"#,
    );

    smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("abort").or(predicate::str::contains("Error")));
}

// ── Test 5: JSON output ─────────────────────────────────────────────

#[test]
fn orchestrate_json_output() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "json-test"

[[session]]
name = "session-one"
task = "Write one file"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add one.txt"
files = [{ path = "one.txt", content = "one\n" }]

[[session]]
name = "session-two"
task = "Write two file"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add two.txt"
files = [{ path = "two.txt", content = "two\n" }]
"#,
    );

    let assert = smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap(), "--json"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    // Parse as JSON to verify structure
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect(&format!("stdout should be valid JSON, got: {stdout}"));

    assert!(value["run_id"].is_string(), "should have run_id");
    assert!(
        value["session_results"].is_object(),
        "should have session_results"
    );
    assert!(
        value["merge_report"].is_object(),
        "should have merge_report (non-null)"
    );
    assert!(value["elapsed_secs"].is_number(), "should have elapsed_secs");
    assert_eq!(value["manifest_name"], "json-test");

    // Verify session results
    let results = value["session_results"].as_object().unwrap();
    assert!(
        results.contains_key("session-one"),
        "should contain session-one"
    );
    assert!(
        results.contains_key("session-two"),
        "should contain session-two"
    );
}

// ── Test 6: Diamond dependency ──────────────────────────────────────

#[test]
fn orchestrate_diamond_dependency() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "diamond"

[[session]]
name = "root"
task = "Root session"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add root.txt"
files = [{ path = "root.txt", content = "root\n" }]

[[session]]
name = "left"
task = "Left branch"
depends_on = ["root"]

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add left.txt"
files = [{ path = "left.txt", content = "left\n" }]

[[session]]
name = "right"
task = "Right branch"
depends_on = ["root"]

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add right.txt"
files = [{ path = "right.txt", content = "right\n" }]

[[session]]
name = "join"
task = "Join session"
depends_on = ["left", "right"]

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add join.txt"
files = [{ path = "join.txt", content = "join\n" }]
"#,
    );

    let assert = smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    for name in &["root", "left", "right", "join"] {
        assert!(
            stdout.contains(name),
            "stdout should contain {name}, got: {stdout}"
        );
    }

    // Verify merged branch has all files
    let output = git_in(
        &repo,
        tmp.path(),
        &["ls-tree", "--name-only", "smelt/merge/diamond"],
    );
    let files = String::from_utf8_lossy(&output.stdout);
    for f in &["root.txt", "left.txt", "right.txt", "join.txt"] {
        assert!(
            files.contains(f),
            "merged branch should have {f}, got: {files}"
        );
    }
}

// ── Test 7: Implicit sequential (parallel_by_default=false) ─────────

#[test]
fn orchestrate_implicit_sequential() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "implicit-seq"
parallel_by_default = false

[[session]]
name = "first"
task = "First session"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add first.txt"
files = [{ path = "first.txt", content = "first\n" }]

[[session]]
name = "second"
task = "Second session"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add second.txt"
files = [{ path = "second.txt", content = "second\n" }]

[[session]]
name = "third"
task = "Third session"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add third.txt"
files = [{ path = "third.txt", content = "third\n" }]
"#,
    );

    let assert = smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    for name in &["first", "second", "third"] {
        assert!(
            stdout.contains(name),
            "stdout should contain {name}, got: {stdout}"
        );
    }
    assert!(
        stdout.contains("done"),
        "all sessions should be done, got: {stdout}"
    );

    // Verify merged branch exists
    let output = git_in(&repo, tmp.path(), &["branch", "--list", "smelt/merge/*"]);
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(
        branches.contains("smelt/merge/implicit-seq"),
        "merged branch should exist, got: {branches}"
    );
}
