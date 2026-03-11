//! Integration tests for agent session support.
//!
//! Tests marked `#[ignore]` require the `claude` CLI on PATH and will be
//! skipped in CI without Claude Code installed.
//!
//! Non-ignored tests verify error handling, preflight logic, and graceful
//! degradation — they run in all environments.

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

// ── Tests requiring Claude Code CLI (ignored by default) ────────────

#[test]
#[ignore = "Requires Claude Code CLI on PATH"]
fn agent_executor_spawns_and_completes() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "agent-spawn"
base_ref = "HEAD"

[[session]]
name = "hello-agent"
task = "Create a file called hello.txt with the content 'Hello from Claude'. Commit your change."
file_scope = ["hello.txt"]
timeout_secs = 120
"#,
    );

    let assert = smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .timeout(std::time::Duration::from_secs(180))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("hello-agent"),
        "stdout should contain session name, got: {stdout}"
    );

    // Verify the merged branch exists
    let output = std::process::Command::new("git")
        .args(["branch", "--list", "smelt/merge/*"])
        .current_dir(&repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", tmp.path())
        .output()
        .expect("git branch");
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(
        branches.contains("smelt/merge/agent-spawn"),
        "merged branch should exist, got: {branches}"
    );

    // Verify log file was written
    let smelt_dir = repo.join(".smelt");
    let has_log = walkdir(smelt_dir.join("runs"))
        .any(|p| p.extension().is_some_and(|e| e == "log"));
    assert!(has_log, "log file should exist under .smelt/runs/");
}

/// Walk a directory recursively and yield file paths.
fn walkdir(root: std::path::PathBuf) -> impl Iterator<Item = std::path::PathBuf> {
    let mut dir_stack = vec![root];
    let mut file_buf: Vec<std::path::PathBuf> = Vec::new();
    std::iter::from_fn(move || {
        loop {
            if let Some(file) = file_buf.pop() {
                return Some(file);
            }
            let dir = dir_stack.pop()?;
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        dir_stack.push(path);
                    } else {
                        file_buf.push(path);
                    }
                }
            }
        }
    })
}

#[test]
#[ignore = "Requires Claude Code CLI on PATH"]
fn agent_executor_timeout_kills_process() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "agent-timeout"
base_ref = "HEAD"

[[session]]
name = "long-task"
task = "Implement a complete web framework from scratch with 100+ files covering routing, middleware, templating, ORM, and authentication. Each file should be at least 200 lines."
timeout_secs = 5
"#,
    );

    // The orchestration should fail (timeout causes session failure)
    smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap(), "--json"])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .code(1);
}

#[test]
#[ignore = "Requires Claude Code CLI on PATH"]
fn agent_two_sessions_merge() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "agent-two-merge"
base_ref = "HEAD"

[[session]]
name = "add-greeting"
task = "Create a file called greeting.txt with the content 'Hello from Smelt!'. Commit your change."
file_scope = ["greeting.txt"]
timeout_secs = 120

[[session]]
name = "add-farewell"
task = "Create a file called farewell.txt with the content 'Goodbye from Smelt!'. Commit your change."
file_scope = ["farewell.txt"]
timeout_secs = 120
"#,
    );

    let assert = smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .timeout(std::time::Duration::from_secs(300))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("add-greeting"),
        "should show add-greeting session, got: {stdout}"
    );
    assert!(
        stdout.contains("add-farewell"),
        "should show add-farewell session, got: {stdout}"
    );

    // Verify merged branch has both files
    let output = std::process::Command::new("git")
        .args(["ls-tree", "--name-only", "smelt/merge/agent-two-merge"])
        .current_dir(&repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", tmp.path())
        .output()
        .expect("git ls-tree");
    let files = String::from_utf8_lossy(&output.stdout);
    assert!(
        files.contains("greeting.txt"),
        "merged branch should have greeting.txt, got: {files}"
    );
    assert!(
        files.contains("farewell.txt"),
        "merged branch should have farewell.txt, got: {files}"
    );
}

#[test]
#[ignore = "Requires Claude Code CLI on PATH"]
fn agent_session_injects_claude_md_and_settings() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "agent-inject"
base_ref = "HEAD"

[[session]]
name = "verify-inject"
task = "List the files CLAUDE.md and .claude/settings.json, then create a file called injected-proof.txt containing 'injection verified'. Commit your change."
file_scope = ["injected-proof.txt"]
timeout_secs = 120
"#,
    );

    smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .timeout(std::time::Duration::from_secs(180))
        .assert()
        .success();
}

#[test]
#[ignore = "Requires Claude Code CLI on PATH"]
fn agent_session_log_files_written() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "agent-logs"
base_ref = "HEAD"

[[session]]
name = "log-check"
task = "Create a file called logtest.txt with the content 'testing logs'. Commit your change."
file_scope = ["logtest.txt"]
timeout_secs = 120
"#,
    );

    smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .timeout(std::time::Duration::from_secs(180))
        .assert()
        .success();

    // Look for log files under .smelt/runs/
    let runs_dir = repo.join(".smelt/runs");
    assert!(runs_dir.exists(), ".smelt/runs should exist");

    let has_log = walkdir(runs_dir).any(|p| {
        p.extension().is_some_and(|e| e == "log")
    });
    assert!(has_log, "at least one .log file should exist under .smelt/runs/");
}

// ── Tests that do NOT require Claude Code CLI ───────────────────────

#[test]
fn agent_not_found_error_is_actionable() {
    // Verify the AgentNotFound error variant produces a useful message
    let err = smelt_core::SmeltError::AgentNotFound;
    let msg = err.to_string();
    assert!(
        msg.contains("claude"),
        "error should mention 'claude', got: {msg}"
    );
    assert!(
        msg.contains("not found") || msg.contains("Install"),
        "error should be actionable, got: {msg}"
    );
}

#[test]
fn orchestrator_preflight_skips_when_all_scripted() {
    // A manifest with only scripted sessions should not require claude on PATH.
    // We verify this by running a scripted-only orchestration — if preflight
    // checked for claude unconditionally, it would fail on machines without it.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "scripted-only"

[[session]]
name = "scripted-session"
task = "Write a file"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add file.txt"
files = [{ path = "file.txt", content = "scripted content\n" }]
"#,
    );

    // This should succeed regardless of whether claude is installed,
    // because all sessions are scripted and preflight should skip
    // the claude binary check.
    smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn session_runner_graceful_degradation_no_claude() {
    // A manifest with a script=None session run via `smelt session run`
    // should degrade gracefully to Completed (no commits) when claude
    // is not on PATH. This verifies backward compatibility.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "degradation-test"

[[session]]
name = "agent-session"
task = "A real agent task"

[[session]]
name = "scripted-session"
task = "Write a file"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add file.txt"
files = [{ path = "file.txt", content = "scripted content\n" }]
"#,
    );

    // `smelt session run` uses SessionRunner which degrades gracefully.
    // The agent session completes with no commits; the scripted session
    // works normally.
    smelt_cmd(&repo)
        .args(["session", "run", manifest.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn orchestrate_agent_session_without_claude_shows_install_message() {
    // When orchestrating with agent sessions and claude is NOT on PATH,
    // the CLI should exit with a clear error message about installation.
    // We simulate this by using a PATH that definitely lacks claude.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "no-claude-test"

[[session]]
name = "agent-session"
task = "A real agent task"
"#,
    );

    // Override PATH to exclude claude (keep only git and basic tools)
    let git_path = which::which("git").expect("git on PATH");
    let git_dir = git_path.parent().expect("git parent dir");
    // Build a minimal PATH with just the directory containing git
    let minimal_path = format!(
        "{}:/usr/bin:/bin",
        git_dir.display()
    );

    let mut cmd = smelt_cmd(&repo);
    cmd.env("PATH", &minimal_path)
        .args(["orchestrate", "run", manifest.to_str().unwrap()]);

    // Should fail because claude is not found
    cmd.assert()
        .code(1)
        .stderr(
            predicate::str::contains("claude")
                .or(predicate::str::contains("not found"))
                .or(predicate::str::contains("Install")),
        );
}

#[test]
fn agent_manifest_example_parses() {
    // Verify the example manifest at examples/agent-manifest.toml is valid.
    let manifest_content =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/agent-manifest.toml"))
            .expect("read example manifest");
    let manifest = smelt_core::Manifest::parse(&manifest_content)
        .expect("example manifest should parse");

    assert_eq!(manifest.manifest.name, "agent-demo");
    assert_eq!(manifest.sessions.len(), 2);
    assert_eq!(manifest.sessions[0].name, "add-greeting");
    assert_eq!(manifest.sessions[1].name, "add-farewell");

    // Both sessions should be agent sessions (no script)
    assert!(
        manifest.sessions[0].script.is_none(),
        "add-greeting should be an agent session"
    );
    assert!(
        manifest.sessions[1].script.is_none(),
        "add-farewell should be an agent session"
    );

    // Both should have file_scope
    assert!(manifest.sessions[0].file_scope.is_some());
    assert!(manifest.sessions[1].file_scope.is_some());

    // Both should have timeout
    assert_eq!(manifest.sessions[0].timeout_secs, Some(120));
    assert_eq!(manifest.sessions[1].timeout_secs, Some(120));
}
