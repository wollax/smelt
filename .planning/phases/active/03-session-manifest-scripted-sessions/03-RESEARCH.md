# Phase 3: Session Manifest & Scripted Sessions - Research

**Researched:** 2026-03-09
**Domain:** TOML manifest parsing, process lifecycle management, declarative scripted sessions
**Confidence:** HIGH

## Summary

This phase introduces two primary constructs: a TOML-based session manifest that defines what each agent session should work on, and a scripted session backend that executes predefined commit sequences in worktrees for deterministic testing of the merge pipeline.

The standard approach uses the existing `serde` + `toml` stack (already in workspace dependencies) for manifest parsing, `tokio::process::Command` with `std::os::unix::process::CommandExt::process_group(0)` for process group isolation, and `tokio::select!` with `tokio::time::timeout` for completion/timeout detection. No new heavyweight dependencies are needed — the project already has `toml`, `serde`, `tokio` (with `process` feature), `libc`, and `chrono`.

The scripted session backend is an internal Rust executor (not a spawned external process) that runs inside a tokio task. It reads the script definition from the manifest, shells out to git in the worktree to create commits according to the script, and simulates failures by exiting early or hanging. For parallel execution testing, each scripted session runs in its own tokio task.

**Primary recommendation:** Keep the scripted session as an in-process executor using the existing `GitOps` trait, not a separate binary. This eliminates process group complexity for the scripted backend while the process management infrastructure is built for future real-agent sessions.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
| --- | --- | --- | --- |
| serde | 1 (workspace) | Manifest struct derive | Already in use for WorktreeState |
| toml | 1 (workspace) | Manifest file parsing | Already in use for state files |
| tokio | 1 (workspace, `process` + `rt-multi-thread` + `macros`) | Async runtime, process spawning, task management | Already in use project-wide |
| chrono | 0.4 (workspace) | Timestamps for session results | Already in use for WorktreeState |
| libc | 0.2 (workspace) | Low-level POSIX helpers (if needed beyond std) | Already a dependency |

### Supporting
| Library | Version | Purpose | When to Use |
| --- | --- | --- | --- |
| globset | 0.4 | Validate/compile glob patterns in `file_scope` | Only if manifest loading validates globs at parse time |
| tokio::signal | (part of tokio) | Ctrl-C / SIGTERM handler for graceful shutdown | Orchestrator shutdown sequence |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
| --- | --- | --- |
| `globset` for glob validation | Store globs as raw strings, validate later | Simpler manifest parsing, but errors surface later at session execution time |
| In-process scripted executor | Separate `smelt-script-runner` binary | Would exercise process group management early, but adds unnecessary complexity for deterministic test scenarios |
| `nix` crate for process groups | `std::os::unix::process::CommandExt::process_group()` | `process_group(0)` is stable since Rust 1.64, no need for `nix` crate |
| `setsid()` for session isolation | `process_group(0)` | `setsid` is nightly-only (tracking issue #105376); `process_group(0)` is stable and sufficient |

**Installation:**
```bash
# globset only — everything else is already in workspace
cargo add globset@0.4 --package smelt-core  # optional, only if validating globs at parse time
```

## Architecture Patterns

### Recommended Project Structure
```
crates/smelt-core/src/
├── session/
│   ├── mod.rs              # Re-exports, SessionManager
│   ├── manifest.rs         # Manifest types, parsing, validation
│   ├── script.rs           # ScriptExecutor — runs scripted sessions
│   ├── runner.rs           # SessionRunner — orchestrates session lifecycle
│   └── types.rs            # SessionResult, SessionOutcome, completion types
├── git/
│   ├── mod.rs              # GitOps trait (add commit_files, rev_list methods)
│   └── cli.rs              # GitCli implementation
├── worktree/               # (existing, unchanged)
└── error.rs                # Add session-related error variants
```

### Pattern 1: Manifest as Typed TOML with Serde
**What:** Define the manifest as a hierarchy of Rust structs with `#[derive(Deserialize, Serialize)]` and load via `toml::from_str`.
**When to use:** Always — this is the only manifest format.
**Example:**
```toml
# smelt-manifest.toml
[manifest]
name = "feature-auth"
base_ref = "main"                    # default for all sessions

[[session]]
name = "add-login"
task = "Implement login endpoint"
file_scope = ["src/auth/**", "src/lib.rs"]
timeout_secs = 300

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "Add login handler"
files = [
  { path = "src/auth/login.rs", content = "pub fn login() {}\n" },
  { path = "src/lib.rs", content = "pub mod auth;\n" },
]

[[session.script.steps]]
action = "commit"
message = "Add login tests"
files = [
  { path = "src/auth/login_test.rs", content = "// tests\n" },
]

[[session]]
name = "add-signup"
task = "Implement signup endpoint"
file_scope = ["src/auth/**", "src/lib.rs"]

[session.script]
backend = "scripted"
exit_after = 1                       # simulate crash after 1 commit
simulate_failure = "crash"           # non-zero exit

[[session.script.steps]]
action = "commit"
message = "Add signup handler"
files = [
  { path = "src/auth/signup.rs", content = "pub fn signup() {}\n" },
]

[[session.script.steps]]
action = "commit"
message = "This commit should not happen"
files = [
  { path = "src/auth/signup.rs", content = "pub fn signup() { todo!() }\n" },
]
```

**Rust types:**
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub manifest: ManifestMeta,
    #[serde(rename = "session")]
    pub sessions: Vec<SessionDef>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ManifestMeta {
    pub name: String,
    #[serde(default = "default_base_ref")]
    pub base_ref: String,
}

fn default_base_ref() -> String { "HEAD".to_string() }

#[derive(Debug, Deserialize, Serialize)]
pub struct SessionDef {
    pub name: String,
    /// Inline task description.
    pub task: Option<String>,
    /// Path to external task description file.
    pub task_file: Option<String>,
    /// Glob patterns and exact paths for file scope.
    pub file_scope: Option<Vec<String>>,
    /// Base ref override for this session.
    pub base_ref: Option<String>,
    /// Timeout in seconds (None = use orchestrator default).
    pub timeout_secs: Option<u64>,
    /// Environment variable overrides.
    pub env: Option<std::collections::HashMap<String, String>>,
    /// Script definition (required for scripted backend).
    pub script: Option<ScriptDef>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ScriptDef {
    pub backend: String,  // "scripted" for now
    pub exit_after: Option<usize>,
    pub simulate_failure: Option<FailureMode>,
    pub steps: Vec<ScriptStep>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureMode {
    Crash,
    Hang,
    Partial,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ScriptStep {
    Commit {
        message: String,
        files: Vec<FileChange>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FileChange {
    pub path: String,
    /// Inline content (mutually exclusive with content_file).
    pub content: Option<String>,
    /// Path to file containing content.
    pub content_file: Option<String>,
}
```

### Pattern 2: In-Process Script Executor
**What:** The scripted session executor runs inside the orchestrator process as a tokio task, using `GitOps` to create commits in the worktree.
**When to use:** All scripted sessions — they do not spawn external processes.
**Why:** Scripted sessions are deterministic test fixtures. They don't need process isolation because they ARE the test infrastructure. Process group management is only needed for real agent sessions (future phase).
**Example:**
```rust
pub struct ScriptExecutor<G: GitOps> {
    git: G,
    worktree_path: PathBuf,
}

impl<G: GitOps> ScriptExecutor<G> {
    pub async fn execute(
        &self,
        script: &ScriptDef,
    ) -> SessionResult {
        let mut completed_steps = 0;

        for (i, step) in script.steps.iter().enumerate() {
            // Check exit_after
            if let Some(exit_after) = script.exit_after {
                if i >= exit_after {
                    return match script.simulate_failure {
                        Some(FailureMode::Crash) => SessionResult::failed(
                            completed_steps,
                            "simulated crash".to_string(),
                        ),
                        Some(FailureMode::Hang) => {
                            // Sleep forever (orchestrator timeout will kill)
                            tokio::time::sleep(Duration::from_secs(u64::MAX)).await;
                            unreachable!()
                        }
                        Some(FailureMode::Partial) | None => SessionResult::failed(
                            completed_steps,
                            "simulated partial failure".to_string(),
                        ),
                    };
                }
            }

            match step {
                ScriptStep::Commit { message, files } => {
                    self.execute_commit(message, files).await?;
                    completed_steps += 1;
                }
            }
        }

        SessionResult::completed(completed_steps)
    }
}
```

### Pattern 3: Process Management for Real Agent Sessions (Infrastructure)
**What:** When spawning external processes (future real agents), use `process_group(0)` for isolation and `tokio::select!` for timeout management.
**When to use:** Future phase, but infrastructure built now.
**Example:**
```rust
use std::os::unix::process::CommandExt;

let mut cmd = tokio::process::Command::new("agent-binary");
cmd.current_dir(&worktree_path)
   .envs(&session_env)
   .process_group(0)     // New process group (stable since Rust 1.64)
   .kill_on_drop(true);  // Safety net

let mut child = cmd.spawn()?;
let pid = child.id().expect("child should have PID");

// Timeout management
let result = tokio::select! {
    status = child.wait() => {
        match status {
            Ok(exit) => SessionOutcome::Exited(exit.code()),
            Err(e) => SessionOutcome::Error(e.to_string()),
        }
    }
    _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => {
        // Graceful shutdown: SIGTERM -> wait -> SIGKILL
        let pgid = nix::unistd::Pid::from_raw(-(pid as i32));
        // Or use libc::kill directly:
        unsafe { libc::kill(-(pid as i32), libc::SIGTERM); }

        match tokio::time::timeout(
            Duration::from_secs(grace_period),
            child.wait(),
        ).await {
            Ok(Ok(status)) => SessionOutcome::TimedOut,
            _ => {
                unsafe { libc::kill(-(pid as i32), libc::SIGKILL); }
                let _ = child.wait().await;
                SessionOutcome::Killed
            }
        }
    }
};
```

### Pattern 4: Session Result and Completion Signaling
**What:** Two-signal completion: process exit (primary) + branch state verification (secondary).
**When to use:** After every session completes or fails.
**Example:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResult {
    pub outcome: SessionOutcome,
    pub steps_completed: usize,
    pub failure_reason: Option<String>,
    pub has_commits: bool,      // branch state verification
    pub duration: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOutcome {
    Completed,
    Failed,
    TimedOut,
    Killed,
}

// Branch state verification
async fn verify_branch_state<G: GitOps>(
    git: &G,
    branch: &str,
    base_ref: &str,
) -> Result<bool> {
    // Check if branch has commits beyond base_ref
    let output = git.rev_list(base_ref, branch).await?;
    Ok(!output.is_empty())
}
```

### Anti-Patterns to Avoid
- **Mutable global state for session tracking:** Use the existing `WorktreeState` TOML files, updated by the orchestrator only (not by the session process).
- **Session self-reporting:** Sessions must not update their own state files. The orchestrator observes process exit and updates state — this is crash-safe.
- **Blocking git operations in async context:** Always use `tokio::process::Command`, never `std::process::Command` in async code.
- **Using `setsid` via `pre_exec`:** This forces the slow fork/exec path. Use `process_group(0)` instead — it works with `posix_spawn` fast path and is stable.
- **Spawning processes for scripted sessions:** Scripted sessions should be in-process executors. They exist to test the merge pipeline, not to test process management.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
| --- | --- | --- | --- |
| TOML parsing | Custom parser | `toml::from_str` with serde derive | TOML spec is complex; hand-rolling is error-prone |
| Process group management | `pre_exec` with `libc::setsid()` | `CommandExt::process_group(0)` | Stable API since 1.64, uses fast `posix_spawn` path |
| Timeout with cancellation | Manual timer + channel | `tokio::select!` + `tokio::time::sleep` | Built into tokio, handles cancellation correctly |
| Glob pattern matching | Regex-based path matching | `globset::Glob` | Handles `**`, character classes, edge cases correctly |
| Signal escalation (SIGTERM->SIGKILL) | Ad-hoc signal code | Structured shutdown function with `libc::kill(-pgid, sig)` | Negative PID targets process group; standard Unix pattern |
| Process exit code extraction | Platform-specific code | `ExitStatus::code()` + `ExitStatusExt::signal()` | std handles cross-platform differences |

**Key insight:** The scripted session backend is essentially a test harness. It doesn't need process isolation — it uses the same `GitOps` trait as the rest of the system. Process management infrastructure should be built for the *future* real-agent backend, not exercised by the scripted backend.

## Common Pitfalls

### Pitfall 1: TOML Array-of-Tables Syntax Confusion
**What goes wrong:** Using `[session]` (single table) instead of `[[session]]` (array of tables) when there are multiple sessions.
**Why it happens:** TOML's `[[section]]` syntax for arrays of tables is less intuitive than JSON arrays.
**How to avoid:** Manifest struct uses `#[serde(rename = "session")] pub sessions: Vec<SessionDef>` — serde+toml handles `[[session]]` correctly. Add a validation step that checks `sessions.len() >= 1` after parsing.
**Warning signs:** Deserialization error mentioning "expected array, found table."

### Pitfall 2: Serde Enum Representation in TOML
**What goes wrong:** TOML does not support tuple variants with internally tagged enums (`#[serde(tag = "type")]` on an enum with tuple variants causes a compile error).
**Why it happens:** TOML's data model is more restrictive than JSON.
**How to avoid:** For `ScriptStep`, use `#[serde(tag = "action")]` with struct variants only (no tuple variants). For `FailureMode`, use `#[serde(rename_all = "snake_case")]` as a simple string enum — TOML handles this fine.
**Warning signs:** Compile-time error about unsupported serde representation.

### Pitfall 3: Process Group Signal Delivery
**What goes wrong:** Sending SIGTERM to the child PID only, leaving grandchild processes alive.
**Why it happens:** Forgetting to negate the PID when calling `kill()` to target the process group.
**How to avoid:** Always use `libc::kill(-pgid, signal)` (negative PID) to signal the entire process group. Document this in the code with a comment explaining the negative PID convention.
**Warning signs:** Zombie processes after orchestrator shutdown, `ps aux | grep smelt` showing orphaned children.

### Pitfall 4: Tokio Task Cancellation vs Process Cleanup
**What goes wrong:** Aborting a tokio task that owns a `Child` handle without waiting for the process to exit, creating zombies.
**Why it happens:** `kill_on_drop(true)` sends SIGKILL but doesn't wait for the process. The background reaper is best-effort.
**How to avoid:** Always explicitly `child.wait().await` after sending signals. Use a cleanup function that runs even on task cancellation (e.g., via `Drop` or a shutdown channel).
**Warning signs:** Zombie processes accumulating during test runs.

### Pitfall 5: Git Operations in Wrong Directory
**What goes wrong:** Running git commands in the repo root instead of the worktree directory.
**Why it happens:** The existing `GitCli` always runs in `self.repo_root`. Scripted sessions need to commit in their worktree.
**How to avoid:** Either create a new `GitCli` instance pointed at the worktree path, or add worktree-aware methods to `GitOps` that accept a `working_dir` parameter. The former is cleaner — `GitCli::new(git_binary, worktree_path)`.
**Warning signs:** Commits appearing on the main repo branch instead of the session branch.

### Pitfall 6: File Content Race in Worktree
**What goes wrong:** Writing files and running `git add` + `git commit` without ensuring the write is flushed to disk.
**Why it happens:** Async file I/O or buffered writes may not have flushed before git reads the file.
**How to avoid:** Use synchronous `std::fs::write` (which is atomic on most filesystems) inside a `tokio::task::spawn_blocking` if needed, or just use sync I/O directly since file writes in worktrees are fast and infrequent.
**Warning signs:** Intermittent test failures where commits have empty files.

## Code Examples

### Loading and Validating a Manifest
```rust
// Source: project conventions + serde/toml documentation
use std::path::Path;

impl Manifest {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| SmeltError::io("reading manifest", path, e))?;
        let manifest: Manifest = toml::from_str(&content)
            .map_err(|e| SmeltError::ManifestParse(format!("{}: {e}", path.display())))?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<()> {
        if self.sessions.is_empty() {
            return Err(SmeltError::ManifestValidation(
                "manifest must define at least one session".to_string(),
            ));
        }

        let mut names = std::collections::HashSet::new();
        for session in &self.sessions {
            if !names.insert(&session.name) {
                return Err(SmeltError::ManifestValidation(
                    format!("duplicate session name: '{}'", session.name),
                ));
            }
            // Validate session name is a valid branch component
            if session.name.contains('/') || session.name.contains(' ') {
                return Err(SmeltError::ManifestValidation(
                    format!("session name '{}' contains invalid characters", session.name),
                ));
            }
        }
        Ok(())
    }
}
```

### Creating Commits in a Worktree (Scripted Session)
```rust
// Source: existing GitCli pattern + git CLI conventions
impl<G: GitOps> ScriptExecutor<G> {
    async fn execute_commit(
        &self,
        message: &str,
        files: &[FileChange],
    ) -> Result<()> {
        // 1. Write files to worktree
        for file in files {
            let full_path = self.worktree_path.join(&file.path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| SmeltError::io("creating directory", parent, e))?;
            }
            let content = match (&file.content, &file.content_file) {
                (_, Some(cf)) => std::fs::read_to_string(cf)
                    .map_err(|e| SmeltError::io("reading content file", cf.as_ref(), e))?,
                (Some(c), _) => c.clone(),
                (None, None) => String::new(),
            };
            std::fs::write(&full_path, &content)
                .map_err(|e| SmeltError::io("writing file", &full_path, e))?;
        }

        // 2. Stage files
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        self.git.add(&paths).await?;

        // 3. Commit
        self.git.commit(message).await?;

        Ok(())
    }
}
```

### Graceful Shutdown Sequence
```rust
// Source: tokio docs + Unix process group conventions
use std::time::Duration;

const DEFAULT_GRACE_PERIOD_SECS: u64 = 5;

async fn shutdown_process_group(
    child: &mut tokio::process::Child,
    grace_period: Duration,
) -> std::io::Result<std::process::ExitStatus> {
    let pid = child.id().expect("child must have PID");
    let pgid = -(pid as i32);

    // 1. SIGTERM to process group
    unsafe { libc::kill(pgid, libc::SIGTERM); }

    // 2. Wait for graceful exit
    match tokio::time::timeout(grace_period, child.wait()).await {
        Ok(result) => result,
        Err(_) => {
            // 3. Grace period expired — SIGKILL
            unsafe { libc::kill(pgid, libc::SIGKILL); }
            child.wait().await
        }
    }
}
```

### GitOps Additions for Session Support
```rust
// New methods needed on GitOps trait
pub trait GitOps {
    // ... existing methods ...

    /// Stage files for commit (git add).
    fn add(&self, paths: &[&str]) -> impl Future<Output = Result<()>> + Send;

    /// Create a commit with the given message.
    fn commit(&self, message: &str) -> impl Future<Output = Result<()>> + Send;

    /// List commits between base_ref and branch (git rev-list base..branch).
    fn rev_list(
        &self,
        base: &str,
        branch: &str,
    ) -> impl Future<Output = Result<Vec<String>>> + Send;
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
| --- | --- | --- | --- |
| `pre_exec` + `libc::setsid()` for process groups | `CommandExt::process_group(0)` | Rust 1.64 (Sept 2022) | No need for unsafe `pre_exec`; uses fast `posix_spawn` path |
| `nix` crate for signal sending | `libc::kill()` directly | Always available | `nix` is overkill if only using `kill()` — `libc` is already a dependency |
| External TOML files per session | Single manifest TOML with `[[session]]` | Design decision | Captures cross-session relationships; `serde` handles `[[array_of_tables]]` natively |
| Shell scripts for test scenarios | Declarative TOML script definitions | Design decision | Deterministic, reproducible, no shell injection risks |

**Deprecated/outdated:**
- `setsid()` via `CommandExt`: Still nightly-only (tracking issue rust-lang/rust#105376). Use `process_group(0)` instead.
- `pre_exec` for setting process group: Forces slow fork/exec path. `process_group(0)` avoids this.

## Open Questions

1. **GitOps trait extension strategy**
   - What we know: `ScriptExecutor` needs `add()`, `commit()`, and `rev_list()` on `GitOps`.
   - What's unclear: Whether to extend the existing `GitOps` trait or create a separate `GitSessionOps` trait.
   - Recommendation: Extend `GitOps` directly — these are fundamental git operations that any backend will need. Keep the trait cohesive.

2. **Scripted session executor: in-process vs subprocess**
   - What we know: In-process is simpler and exercises the `GitOps` trait directly. Subprocess would test process management earlier.
   - What's unclear: Whether future phases will need the process management infrastructure built and tested now.
   - Recommendation: In-process executor for scripted sessions. Build process management infrastructure as a separate module that can be tested independently with a simple test binary, then used by real-agent sessions in a later phase.

3. **Hang simulation duration**
   - What we know: `FailureMode::Hang` needs to block until the orchestrator's timeout kills it.
   - What's unclear: Whether `tokio::time::sleep(Duration::MAX)` is safe or whether it should use a cancellation token.
   - Recommendation: Use `tokio::sync::Notify` — the executor awaits notification that never comes. The orchestrator aborts the tokio task, which cancels the await. Alternatively, `sleep(Duration::from_secs(86400))` with a comment is simpler and practically equivalent.

## Sources

### Primary (HIGH confidence)
- Context7 `/websites/rs_tokio_1_49_0` — tokio::process::Command, kill_on_drop, JoinSet, select!, timeout
- Context7 `/websites/serde_rs` — enum representations (tagged, untagged, adjacently tagged), rename_all
- Context7 `/nix-rust/nix` — signal handling patterns, process management
- Context7 `/websites/rs_toml` — TOML deserialization, table/array-of-tables handling
- [std::os::unix::process::CommandExt](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html) — `process_group()` stable since 1.64, `setsid()` nightly-only
- [globset 0.4 docs](https://docs.rs/globset/latest/globset/) — glob pattern compilation and matching

### Secondary (MEDIUM confidence)
- [RFC 3228](https://rust-lang.github.io/rfcs/3228-process-process_group.html) — process_group rationale and design
- [tokio issue #4312](https://github.com/tokio-rs/tokio/issues/4312) — discussion on process group creation with tokio

### Tertiary (LOW confidence)
- None — all findings verified with primary sources.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all libraries are already workspace dependencies; APIs verified via Context7 and official docs
- Architecture: HIGH — follows established project patterns (GitOps trait, WorktreeState, serde+toml); in-process executor is straightforward
- Pitfalls: HIGH — verified via official docs (process_group stability, serde enum limitations, tokio kill_on_drop caveats)
- Process management: MEDIUM — infrastructure patterns verified but will only be fully exercised in future real-agent phase

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable ecosystem, 30-day validity)
