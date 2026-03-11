//! Integration tests for `smelt summary` and orchestrate summary output.
//!
//! Tests verify end-to-end behavior of summary table display, scope violation
//! reporting, and JSON output through the CLI binary.

use assert_cmd::Command;

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

// ── Test 1: Orchestrate shows summary table ─────────────────────────

#[test]
fn orchestrate_shows_summary_table() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "summary-table"

[[session]]
name = "alpha"
task = "Write alpha files"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add alpha.txt"
files = [{ path = "alpha.txt", content = "alpha content\nline 2\n" }]

[[session]]
name = "beta"
task = "Write beta files"

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

    // Summary table should appear with session names and columns
    assert!(
        stdout.contains("Summary"),
        "output should contain Summary header, got: {stdout}"
    );
    assert!(
        stdout.contains("alpha"),
        "summary should contain session alpha, got: {stdout}"
    );
    assert!(
        stdout.contains("beta"),
        "summary should contain session beta, got: {stdout}"
    );
    assert!(
        stdout.contains("+Lines"),
        "summary should contain +Lines column, got: {stdout}"
    );
    assert!(
        stdout.contains("-Lines"),
        "summary should contain -Lines column, got: {stdout}"
    );
    assert!(
        stdout.contains("Files"),
        "summary should contain Files column, got: {stdout}"
    );
    assert!(
        stdout.contains("Total"),
        "summary should contain Total row, got: {stdout}"
    );
}

// ── Test 2: Orchestrate shows scope violations ──────────────────────

#[test]
fn orchestrate_shows_scope_violations() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "scope-test"

[[session]]
name = "scoped-session"
task = "Write files with scope"
file_scope = ["docs/**"]

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add files"
files = [
    { path = "docs/guide.md", content = "Guide content\n" },
    { path = "src/main.rs", content = "fn main() { }\n" },
]
"#,
    );

    let assert = smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    // Violations section should appear
    assert!(
        stdout.contains("Scope violations"),
        "output should show scope violations section, got: {stdout}"
    );
    assert!(
        stdout.contains("src/main.rs"),
        "should list the out-of-scope file, got: {stdout}"
    );
    assert!(
        stdout.contains("outside scope"),
        "should use neutral 'outside scope' phrasing, got: {stdout}"
    );
}

// ── Test 3: No violations section when all files in scope ───────────

#[test]
fn orchestrate_no_violations_section_when_clean() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "clean-scope"

[[session]]
name = "in-scope"
task = "Write files within scope"
file_scope = ["src/**"]

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add source file"
files = [{ path = "src/lib.rs", content = "pub fn hello() {}\n" }]
"#,
    );

    let assert = smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        !stdout.contains("Scope violations"),
        "should NOT show violations section when all in scope, got: {stdout}"
    );
    assert!(
        stdout.contains("Summary"),
        "should still show summary table, got: {stdout}"
    );
}

// ── Test 4: No violations when no file_scope defined ────────────────

#[test]
fn orchestrate_no_violations_when_no_file_scope() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "no-scope"

[[session]]
name = "unscoped"
task = "Write anything"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add files"
files = [
    { path = "anywhere.txt", content = "content\n" },
    { path = "deep/nested/file.txt", content = "nested\n" },
]
"#,
    );

    let assert = smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        !stdout.contains("Scope violations"),
        "no file_scope means no violations, got: {stdout}"
    );
}

// ── Test 5: shared_files not flagged ────────────────────────────────

#[test]
fn orchestrate_shared_files_not_flagged() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "shared-files"
shared_files = ["Cargo.toml", "Cargo.lock"]

[[session]]
name = "feature"
task = "Add feature with shared file changes"
file_scope = ["src/**"]

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add feature"
files = [
    { path = "src/feature.rs", content = "pub fn feature() {}\n" },
    { path = "Cargo.toml", content = "[package]\nname = \"test\"\n" },
]
"#,
    );

    let assert = smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        !stdout.contains("Scope violations"),
        "shared_files should not be flagged as violations, got: {stdout}"
    );
    assert!(
        stdout.contains("Summary"),
        "should still show summary, got: {stdout}"
    );
}

// ── Test 6: JSON output includes summary ────────────────────────────

#[test]
fn orchestrate_json_includes_summary() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "json-summary"

[[session]]
name = "session-a"
task = "Write a file"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add a.txt"
files = [{ path = "a.txt", content = "a content\n" }]
"#,
    );

    let assert = smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap(), "--json"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect(&format!("should be valid JSON, got: {stdout}"));

    // Summary field should be present and non-null
    assert!(
        value["summary"].is_object(),
        "JSON should include summary object, got: {value}"
    );

    let summary = &value["summary"];
    assert_eq!(summary["manifest_name"], "json-summary");
    assert!(
        summary["sessions"].is_array(),
        "summary should have sessions array"
    );
    assert!(
        summary["totals"].is_object(),
        "summary should have totals object"
    );

    let sessions = summary["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["session_name"], "session-a");
}

// ── Test 7: Standalone summary command ──────────────────────────────

#[test]
fn standalone_summary_command() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let repo = setup_test_repo(&tmp);

    smelt_cmd(&repo).arg("init").assert().success();

    let manifest = write_manifest(
        &repo,
        r#"
[manifest]
name = "standalone-test"

[[session]]
name = "writer"
task = "Write a file"

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "add file.txt"
files = [{ path = "file.txt", content = "hello world\n" }]
"#,
    );

    // First, run orchestrate to create the run data
    smelt_cmd(&repo)
        .args(["orchestrate", "run", manifest.to_str().unwrap()])
        .assert()
        .success();

    // Now run standalone summary
    let assert = smelt_cmd(&repo)
        .args(["summary", manifest.to_str().unwrap()])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);

    assert!(
        stdout.contains("Summary"),
        "standalone summary should show summary table, got: {stdout}"
    );
    assert!(
        stdout.contains("writer"),
        "should show session name, got: {stdout}"
    );
    assert!(
        stdout.contains("Files"),
        "should show Files column, got: {stdout}"
    );

    // Also test --json output
    let assert_json = smelt_cmd(&repo)
        .args(["summary", manifest.to_str().unwrap(), "--json"])
        .assert()
        .success();

    let json_stdout = String::from_utf8_lossy(&assert_json.get_output().stdout);
    let value: serde_json::Value =
        serde_json::from_str(&json_stdout).expect("should be valid JSON");
    assert_eq!(value["manifest_name"], "standalone-test");
    assert!(value["sessions"].is_array());
}
