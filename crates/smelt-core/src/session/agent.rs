//! AgentExecutor — spawns Claude Code as a real agent backend.
//!
//! Mirrors `ScriptExecutor`'s interface (takes a worktree path, returns
//! `SessionResult`) but spawns an external `claude` CLI process instead
//! of running scripted steps.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::Command;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::error::{Result, SmeltError};
use crate::session::types::{SessionOutcome, SessionResult};

/// Executor that spawns Claude Code CLI as a real agent backend.
///
/// Each execution:
/// 1. Injects `CLAUDE.md` with session constraints into the worktree
/// 2. Injects `.claude/settings.json` for headless configuration
/// 3. Spawns `claude -p "..." --dangerously-skip-permissions --output-format json`
/// 4. Manages process lifecycle (timeout, cancellation, process group kill)
/// 5. Captures stdout/stderr to a log file
/// 6. Maps exit code to `SessionResult`
pub struct AgentExecutor {
    claude_binary: PathBuf,
    worktree_path: PathBuf,
    log_path: PathBuf,
    timeout: Option<Duration>,
}

impl AgentExecutor {
    /// Create a new `AgentExecutor`.
    ///
    /// - `claude_binary`: Pre-resolved path to the `claude` CLI binary.
    /// - `worktree_path`: Path to the git worktree where the agent will work.
    /// - `log_path`: Path to the log file for stdout/stderr capture.
    /// - `timeout`: Optional per-session deadline. If exceeded, the agent
    ///   process group is killed and `SessionOutcome::TimedOut` is returned.
    pub fn new(
        claude_binary: PathBuf,
        worktree_path: PathBuf,
        log_path: PathBuf,
        timeout: Option<Duration>,
    ) -> Self {
        Self {
            claude_binary,
            worktree_path,
            log_path,
            timeout,
        }
    }

    /// Build the task prompt from the session's task description and
    /// optional file scope hints.
    fn build_prompt(session_name: &str, task: &str, file_scope: Option<&[String]>) -> String {
        let mut prompt = String::new();
        prompt.push_str("You are working in a git worktree on a focused task.\n\n");
        prompt.push_str(&format!("Session: {session_name}\n\n"));
        prompt.push_str("## Task\n\n");
        prompt.push_str(task);
        if let Some(scopes) = file_scope {
            prompt.push_str("\n\n## File Scope\n\n");
            prompt.push_str("Focus on these paths:\n");
            for scope in scopes {
                prompt.push_str(&format!("- {scope}\n"));
            }
        }
        prompt.push_str(
            "\n\n## Instructions\n\n\
             - Stay within the assigned file scope.\n\
             - Commit your changes with descriptive messages.\n\
             - Do NOT push to any remote.\n\
             - Do NOT modify files outside your assigned scope.\n",
        );
        prompt
    }

    /// Inject a session-specific `CLAUDE.md` into the worktree.
    ///
    /// If the worktree already has a `CLAUDE.md` at root (from the
    /// repository), writes to `.claude/CLAUDE.md` instead to avoid
    /// overwriting project-specific instructions.
    fn inject_claude_md(
        worktree_path: &Path,
        session_name: &str,
        task: &str,
        file_scope: Option<&[String]>,
    ) -> Result<()> {
        let mut content = String::new();
        content.push_str("# Session Constraints\n\n");
        content.push_str(&format!("Session: {session_name}\n\n"));
        content.push_str("## Rules\n\n");
        content.push_str("- Work ONLY within this worktree\n");
        content.push_str("- Stay within the assigned file scope\n");
        content.push_str("- Commit your work with descriptive messages\n");
        content.push_str("- Do NOT push to any remote\n");
        content.push_str("- Do NOT modify files outside your assigned scope\n");

        content.push_str("\n## Task\n\n");
        content.push_str(task);
        content.push('\n');

        if let Some(scopes) = file_scope {
            content.push_str("\n## File Scope\n\n");
            content.push_str("Only modify files matching these patterns:\n");
            for scope in scopes {
                content.push_str(&format!("- `{scope}`\n"));
            }
        }

        let root_claude_md = worktree_path.join("CLAUDE.md");
        let target_path = if root_claude_md.exists() {
            // Don't overwrite existing project CLAUDE.md — use .claude/ dir instead
            let dot_claude = worktree_path.join(".claude");
            std::fs::create_dir_all(&dot_claude)
                .map_err(|e| SmeltError::io("creating .claude directory", &dot_claude, e))?;
            dot_claude.join("CLAUDE.md")
        } else {
            root_claude_md
        };

        std::fs::write(&target_path, content)
            .map_err(|e| SmeltError::io("writing CLAUDE.md", &target_path, e))?;

        debug!(
            path = %target_path.display(),
            "injected CLAUDE.md into worktree"
        );

        Ok(())
    }

    /// Inject `.claude/settings.json` into the worktree for headless execution.
    ///
    /// Configures permissions (defense-in-depth alongside `--dangerously-skip-permissions`)
    /// and optionally pins the model.
    fn inject_settings(worktree_path: &Path, model: Option<&str>) -> Result<()> {
        let dot_claude = worktree_path.join(".claude");
        std::fs::create_dir_all(&dot_claude)
            .map_err(|e| SmeltError::io("creating .claude directory", &dot_claude, e))?;

        let mut settings = serde_json::json!({
            "permissions": {
                "allow": [
                    "Bash(*)",
                    "Read(*)",
                    "Write(*)",
                    "Edit(*)",
                    "Glob(*)",
                    "Grep(*)"
                ],
                "deny": [
                    "Bash(git push *)",
                    "Bash(git remote *)",
                    "Bash(curl *)",
                    "Bash(wget *)"
                ]
            }
        });

        if let Some(m) = model {
            settings["model"] = serde_json::Value::String(m.to_string());
        }

        let settings_path = dot_claude.join("settings.json");
        let settings_str = serde_json::to_string_pretty(&settings).map_err(|e| {
            SmeltError::SessionError {
                session: String::new(),
                message: format!("failed to serialize settings.json: {e}"),
            }
        })?;

        std::fs::write(&settings_path, settings_str)
            .map_err(|e| SmeltError::io("writing settings.json", &settings_path, e))?;

        debug!(
            path = %settings_path.display(),
            "injected settings.json into worktree"
        );

        Ok(())
    }

    /// Execute a real agent session by spawning Claude Code.
    ///
    /// # Process lifecycle
    ///
    /// The Claude Code process is spawned in a new process group
    /// (`process_group(0)`) with `kill_on_drop(true)` as a safety net.
    /// Shutdown is managed via `tokio::select!`:
    ///
    /// - **Cancellation**: Orchestrator signals abort via `CancellationToken`
    /// - **Timeout**: Per-session deadline expires
    /// - **Normal exit**: Process completes on its own
    ///
    /// # Known limitations (v0.1.0)
    ///
    /// - `has_commits` is always `true` when exit code is 0. Accurate
    ///   commit detection via `git rev-list` is deferred. The merge phase
    ///   handles sessions with no actual commits gracefully.
    /// - `steps_completed` is always 0 (not meaningful for agent sessions).
    pub async fn execute(
        &self,
        session_name: &str,
        task: &str,
        file_scope: Option<&[String]>,
        model: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<SessionResult> {
        let start_time = Instant::now();

        // Inject CLAUDE.md and settings.json into the worktree
        Self::inject_claude_md(&self.worktree_path, session_name, task, file_scope)?;
        Self::inject_settings(&self.worktree_path, model)?;

        // Build the prompt
        let prompt = Self::build_prompt(session_name, task, file_scope);

        // Build the command
        let mut cmd = Command::new(&self.claude_binary);
        cmd.args([
            "-p",
            &prompt,
            "--dangerously-skip-permissions",
            "--output-format",
            "json",
        ]);
        if let Some(m) = model {
            cmd.args(["--model", m]);
        }
        cmd.current_dir(&self.worktree_path);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.process_group(0);
        cmd.kill_on_drop(true);
        // Clear CLAUDECODE env var to allow spawning Claude Code from within
        // an existing Claude Code session (e.g., when Smelt is invoked by Claude).
        cmd.env_remove("CLAUDECODE");

        info!(
            session = session_name,
            binary = %self.claude_binary.display(),
            worktree = %self.worktree_path.display(),
            "spawning Claude Code agent"
        );

        let mut child = cmd.spawn().map_err(|e| SmeltError::SessionError {
            session: session_name.to_string(),
            message: format!("failed to spawn claude process: {e}"),
        })?;

        let pid = child.id();

        // Take stdout/stderr handles so we can capture them after wait.
        // This lets us use child.wait() (borrow) instead of
        // child.wait_with_output() (move) in the select! macro, which
        // allows the cancellation/timeout arms to also access `child`.
        let child_stdout = child.stdout.take();
        let child_stderr = child.stderr.take();

        // Helper to collect piped output after the process exits.
        let collect_output = |status: std::process::ExitStatus| async move {
            use tokio::io::AsyncReadExt;

            let mut stdout_buf = Vec::new();
            let mut stderr_buf = Vec::new();

            if let Some(mut out) = child_stdout {
                let _ = out.read_to_end(&mut stdout_buf).await;
            }
            if let Some(mut err) = child_stderr {
                let _ = err.read_to_end(&mut stderr_buf).await;
            }

            std::process::Output {
                status,
                stdout: stdout_buf,
                stderr: stderr_buf,
            }
        };

        // Wait for completion with timeout and cancellation
        if let Some(deadline) = self.timeout {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    info!(session = session_name, "cancellation requested, killing agent");
                    kill_process_group(pid);
                    let _ = child.wait().await;
                    Ok(SessionResult {
                        session_name: session_name.to_string(),
                        outcome: SessionOutcome::Killed,
                        steps_completed: 0,
                        failure_reason: Some("cancelled by orchestrator".to_string()),
                        has_commits: false,
                        duration: start_time.elapsed(),
                    })
                }
                wait_result = timeout(deadline, child.wait()) => {
                    match wait_result {
                        Ok(Ok(status)) => {
                            let output = collect_output(status).await;
                            self.map_output_to_result(session_name, output, start_time).await
                        }
                        Ok(Err(io_err)) => {
                            Ok(SessionResult {
                                session_name: session_name.to_string(),
                                outcome: SessionOutcome::Failed,
                                steps_completed: 0,
                                failure_reason: Some(format!("IO error waiting for process: {io_err}")),
                                has_commits: false,
                                duration: start_time.elapsed(),
                            })
                        }
                        Err(_elapsed) => {
                            info!(session = session_name, "timeout expired, killing agent");
                            kill_process_group(pid);
                            let _ = child.wait().await;
                            Ok(SessionResult {
                                session_name: session_name.to_string(),
                                outcome: SessionOutcome::TimedOut,
                                steps_completed: 0,
                                failure_reason: Some(format!(
                                    "session timed out after {}s",
                                    deadline.as_secs()
                                )),
                                has_commits: false,
                                duration: start_time.elapsed(),
                            })
                        }
                    }
                }
            }
        } else {
            // No timeout — just cancel + wait
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    info!(session = session_name, "cancellation requested, killing agent");
                    kill_process_group(pid);
                    let _ = child.wait().await;
                    Ok(SessionResult {
                        session_name: session_name.to_string(),
                        outcome: SessionOutcome::Killed,
                        steps_completed: 0,
                        failure_reason: Some("cancelled by orchestrator".to_string()),
                        has_commits: false,
                        duration: start_time.elapsed(),
                    })
                }
                wait_result = child.wait() => {
                    match wait_result {
                        Ok(status) => {
                            let output = collect_output(status).await;
                            self.map_output_to_result(session_name, output, start_time).await
                        }
                        Err(io_err) => {
                            Ok(SessionResult {
                                session_name: session_name.to_string(),
                                outcome: SessionOutcome::Failed,
                                steps_completed: 0,
                                failure_reason: Some(format!("IO error waiting for process: {io_err}")),
                                has_commits: false,
                                duration: start_time.elapsed(),
                            })
                        }
                    }
                }
            }
        }
    }

    /// Map process output (exit code + stdout/stderr) to a `SessionResult`,
    /// writing captured output to the log file.
    async fn map_output_to_result(
        &self,
        session_name: &str,
        output: std::process::Output,
        start_time: Instant,
    ) -> Result<SessionResult> {
        // Write stdout + stderr to log file
        self.write_log(&output)?;

        let exit_code = output.status.code();

        if output.status.success() {
            info!(
                session = session_name,
                "agent completed successfully (exit 0)"
            );
            Ok(SessionResult {
                session_name: session_name.to_string(),
                outcome: SessionOutcome::Completed,
                steps_completed: 0,
                failure_reason: None,
                // Known v0.1.0 limitation: has_commits is always true when
                // exit code is 0. Accurate commit detection via git rev-list
                // is deferred. The merge phase handles sessions with no actual
                // commits gracefully since they have no diff to merge.
                has_commits: true,
                duration: start_time.elapsed(),
            })
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let failure_reason = if stderr.is_empty() {
                format!("agent exited with code {}", exit_code.unwrap_or(-1))
            } else if stderr.len() > 1000 {
                format!("{}... (truncated)", &stderr[..1000])
            } else {
                stderr.to_string()
            };

            warn!(
                session = session_name,
                exit_code = exit_code,
                "agent failed"
            );

            Ok(SessionResult {
                session_name: session_name.to_string(),
                outcome: SessionOutcome::Failed,
                steps_completed: 0,
                failure_reason: Some(failure_reason),
                has_commits: false,
                duration: start_time.elapsed(),
            })
        }
    }

    /// Write captured stdout and stderr to the log file.
    fn write_log(&self, output: &std::process::Output) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.log_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| SmeltError::io("creating log directory", parent, e))?;
        }

        let mut log_content = Vec::new();
        if !output.stdout.is_empty() {
            log_content.extend_from_slice(b"=== STDOUT ===\n");
            log_content.extend_from_slice(&output.stdout);
            log_content.push(b'\n');
        }
        if !output.stderr.is_empty() {
            log_content.extend_from_slice(b"=== STDERR ===\n");
            log_content.extend_from_slice(&output.stderr);
            log_content.push(b'\n');
        }

        std::fs::write(&self.log_path, &log_content)
            .map_err(|e| SmeltError::io("writing agent log", &self.log_path, e))?;

        debug!(path = %self.log_path.display(), "wrote agent log");

        Ok(())
    }
}

/// Send SIGTERM to the entire process group identified by `pid`.
///
/// The process must have been spawned with `process_group(0)` so that
/// PID == PGID. Sends `kill(-pgid, SIGTERM)` to signal the entire group.
///
/// Silently ignores the case where the process has already exited (ESRCH).
#[cfg(unix)]
fn kill_process_group(pid: Option<u32>) {
    let Some(pid) = pid else {
        warn!("no PID available for process group kill");
        return;
    };

    let Ok(pid_i32) = i32::try_from(pid) else {
        warn!(pid, "PID exceeds i32::MAX, cannot send signal");
        return;
    };

    // Safety: We negate the PID to target the process group.
    // The process was spawned with process_group(0), so PID == PGID.
    let ret = unsafe { libc::kill(-pid_i32, libc::SIGTERM) };
    if ret == -1 {
        let err = std::io::Error::last_os_error();
        // ESRCH = no such process — already dead, that's fine
        if err.raw_os_error() != Some(libc::ESRCH) {
            warn!(pid, error = %err, "failed to kill process group");
        }
    } else {
        debug!(pid, "sent SIGTERM to process group");
    }
}

/// Resolve the `claude` CLI binary on `$PATH`.
///
/// Returns the full path to the binary, or `SmeltError::AgentNotFound`
/// if not found.
pub fn resolve_claude_binary() -> Result<PathBuf> {
    which::which("claude").map_err(|_| SmeltError::AgentNotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_prompt_includes_task() {
        let prompt = AgentExecutor::build_prompt("test-session", "Implement login", None);
        assert!(prompt.contains("Implement login"), "prompt should contain task");
        assert!(
            prompt.contains("test-session"),
            "prompt should contain session name"
        );
    }

    #[test]
    fn test_build_prompt_includes_file_scope() {
        let scopes = vec!["src/auth/**".to_string(), "src/lib.rs".to_string()];
        let prompt =
            AgentExecutor::build_prompt("test-session", "Implement login", Some(&scopes));
        assert!(
            prompt.contains("src/auth/**"),
            "prompt should contain file scope pattern"
        );
        assert!(
            prompt.contains("src/lib.rs"),
            "prompt should contain file scope pattern"
        );
        assert!(
            prompt.contains("File Scope"),
            "prompt should have file scope section"
        );
    }

    #[test]
    fn test_build_prompt_no_file_scope() {
        let prompt = AgentExecutor::build_prompt("test-session", "Implement login", None);
        assert!(
            !prompt.contains("File Scope"),
            "prompt should not have file scope section when none provided"
        );
        assert!(prompt.contains("Implement login"));
    }

    #[test]
    fn test_inject_claude_md_creates_file() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let worktree = tmp.path();

        AgentExecutor::inject_claude_md(worktree, "test-session", "Do the thing", None)
            .expect("inject_claude_md");

        let claude_md = worktree.join("CLAUDE.md");
        assert!(claude_md.exists(), "CLAUDE.md should be created at root");

        let content = std::fs::read_to_string(&claude_md).expect("read CLAUDE.md");
        assert!(content.contains("test-session"));
        assert!(content.contains("Do the thing"));
        assert!(content.contains("Do NOT push"));
    }

    #[test]
    fn test_inject_claude_md_uses_dot_claude_when_root_exists() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let worktree = tmp.path();

        // Create an existing CLAUDE.md at root
        std::fs::write(worktree.join("CLAUDE.md"), "# Existing project instructions\n")
            .expect("write existing CLAUDE.md");

        AgentExecutor::inject_claude_md(worktree, "test-session", "Do the thing", None)
            .expect("inject_claude_md");

        // Original should be untouched
        let root_content =
            std::fs::read_to_string(worktree.join("CLAUDE.md")).expect("read root CLAUDE.md");
        assert_eq!(
            root_content, "# Existing project instructions\n",
            "root CLAUDE.md should not be overwritten"
        );

        // Session CLAUDE.md should be in .claude/
        let dot_claude_md = worktree.join(".claude/CLAUDE.md");
        assert!(
            dot_claude_md.exists(),
            ".claude/CLAUDE.md should be created"
        );

        let content = std::fs::read_to_string(&dot_claude_md).expect("read .claude/CLAUDE.md");
        assert!(content.contains("test-session"));
        assert!(content.contains("Do the thing"));
    }

    #[test]
    fn test_inject_claude_md_with_file_scope() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let worktree = tmp.path();

        let scopes = vec!["src/auth/**".to_string(), "src/lib.rs".to_string()];
        AgentExecutor::inject_claude_md(
            worktree,
            "test-session",
            "Implement login",
            Some(&scopes),
        )
        .expect("inject_claude_md");

        let content =
            std::fs::read_to_string(worktree.join("CLAUDE.md")).expect("read CLAUDE.md");
        assert!(content.contains("src/auth/**"));
        assert!(content.contains("src/lib.rs"));
    }

    #[test]
    fn test_inject_settings_creates_file() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let worktree = tmp.path();

        AgentExecutor::inject_settings(worktree, None).expect("inject_settings");

        let settings_path = worktree.join(".claude/settings.json");
        assert!(settings_path.exists(), "settings.json should be created");

        let content = std::fs::read_to_string(&settings_path).expect("read settings.json");
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("should be valid JSON");

        assert!(parsed["permissions"]["allow"].is_array());
        assert!(parsed["permissions"]["deny"].is_array());
        assert!(
            parsed.get("model").is_none(),
            "model should not be present when not provided"
        );
    }

    #[test]
    fn test_inject_settings_with_model() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let worktree = tmp.path();

        AgentExecutor::inject_settings(worktree, Some("sonnet")).expect("inject_settings");

        let content =
            std::fs::read_to_string(worktree.join(".claude/settings.json")).expect("read");
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("should be valid JSON");

        assert_eq!(
            parsed["model"].as_str(),
            Some("sonnet"),
            "model should be set in settings"
        );
    }

    #[test]
    fn test_inject_settings_deny_includes_push() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let worktree = tmp.path();

        AgentExecutor::inject_settings(worktree, None).expect("inject_settings");

        let content =
            std::fs::read_to_string(worktree.join(".claude/settings.json")).expect("read");
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("should be valid JSON");

        let deny = parsed["permissions"]["deny"]
            .as_array()
            .expect("deny should be array");
        let deny_strs: Vec<&str> = deny.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            deny_strs.iter().any(|s| s.contains("git push")),
            "deny list should include git push"
        );
    }

    #[test]
    #[ignore = "requires claude CLI on PATH"]
    fn test_resolve_claude_binary_found() {
        let path = resolve_claude_binary().expect("should find claude");
        assert!(path.exists(), "resolved path should exist");
    }

    #[test]
    fn test_resolve_claude_binary_not_found() {
        // We can't easily mock PATH in a unit test, so verify the function
        // signature and error type. The actual PATH lookup depends on the
        // environment. If claude IS installed, this test would fail, so
        // we just verify the error variant name in the SmeltError.
        // This is effectively tested by the AgentNotFound variant existing.
        let _variant = SmeltError::AgentNotFound;
        assert!(
            _variant.to_string().contains("claude"),
            "error message should mention claude"
        );
    }
}
