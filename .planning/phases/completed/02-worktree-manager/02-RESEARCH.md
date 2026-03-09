# Phase 2: Worktree Manager - Research

**Researched:** 2026-03-09
**Domain:** Git worktree lifecycle management, Rust CLI patterns
**Confidence:** HIGH

## Summary

This phase wraps `git worktree` CLI operations with Smelt-specific state tracking, orphan detection, and branch lifecycle management. The git worktree subsystem is stable and well-documented — the primary complexity lies in state synchronization between Smelt's TOML state files and git's native worktree metadata, and in safely handling edge cases (dirty worktrees, unmerged branches, dead PIDs).

The existing `SmeltGitOps` trait and `GitCli` shell-out pattern from Phase 1 extend naturally. New worktree-specific git operations (add, remove, list, prune, status) follow the same `Command::new(&git_binary)` async pattern. The `serde` + `toml` crates already in the workspace handle state serialization. Two new dependencies are needed: `chrono` for timestamps and `dialoguer` for interactive confirmation prompts.

**Primary recommendation:** Shell out to `git worktree` for all worktree lifecycle operations. Use `git worktree list --porcelain` for parsing. Track Smelt-specific metadata in per-session TOML files under `.smelt/worktrees/`. Use `libc::kill(pid, 0)` for PID liveness checks — no heavy process crate needed.

## Standard Stack

### Core (already in workspace)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | 1.x | Async runtime, process spawning | Already in workspace, `process` feature enabled |
| serde | 1.x | Serialize/deserialize state TOML | Already in workspace with `derive` |
| toml | 1.x | TOML file read/write | Already in workspace |
| clap | 4.5 | CLI subcommands and aliases | Already in workspace with `derive` |
| thiserror | 2.x | Error variants | Already in workspace |

### New Dependencies
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| chrono | 0.4 | Timestamps (created_at, updated_at) | State file metadata; needs `serde` and `clock` features |
| dialoguer | 0.12 | Interactive confirmation prompts | Destructive operations on dirty worktrees, orphan cleanup |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| chrono | time (crate) | `time` is lighter but `chrono` has better serde integration out-of-box and `DateTime<Utc>` serializes to RFC3339 by default, which is human-readable in TOML |
| dialoguer | inquire | Both work; `dialoguer` is more established (higher download count), simpler API for yes/no prompts |
| libc::kill for PID | sysinfo crate | `sysinfo` is a heavy dependency; `libc::kill(pid, 0)` is 3 lines of unsafe code for a single-purpose check |

**Installation (workspace Cargo.toml additions):**
```toml
chrono = { version = "0.4", features = ["serde", "clock"] }
dialoguer = "0.12"
```

## Architecture Patterns

### Recommended Project Structure
```
crates/smelt-core/src/
├── git/
│   ├── mod.rs           # GitOps trait (extend with worktree methods)
│   └── cli.rs           # GitCli impl (add worktree operations)
├── worktree/
│   ├── mod.rs           # Public API: WorktreeManager
│   ├── state.rs         # WorktreeState TOML serde types
│   └── orphan.rs        # Orphan detection logic
├── error.rs             # Add new error variants
├── init.rs              # Existing
└── lib.rs               # Re-export worktree module

crates/smelt-cli/src/
├── commands/
│   ├── init.rs          # Existing
│   ├── worktree.rs      # smelt worktree {create|list|remove|prune}
│   └── mod.rs           # Register worktree subcommand + wt alias
└── main.rs              # Add Worktree variant to Commands enum
```

### Pattern 1: Extend GitOps Trait for Worktree Operations

**What:** Add worktree-specific methods to the existing `GitOps` trait rather than creating a separate trait.
**When to use:** When operations are fundamentally git commands that benefit from the same test seam.

```rust
// Extend the existing trait
pub trait GitOps {
    // ... existing methods ...

    /// Create a new worktree at `path` on branch `branch_name`, based on `start_point`.
    fn worktree_add(
        &self,
        path: &Path,
        branch_name: &str,
        start_point: &str,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Remove a worktree. If `force` is true, removes even with uncommitted changes.
    fn worktree_remove(
        &self,
        path: &Path,
        force: bool,
    ) -> impl Future<Output = Result<()>> + Send;

    /// List worktrees in porcelain format.
    fn worktree_list(&self) -> impl Future<Output = Result<Vec<GitWorktreeEntry>>> + Send;

    /// Prune stale worktree metadata.
    fn worktree_prune(&self) -> impl Future<Output = Result<()>> + Send;

    /// Delete a branch. `force` = true uses -D (ignores merge status).
    fn branch_delete(
        &self,
        branch_name: &str,
        force: bool,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Check if a branch has unmerged commits relative to a base ref.
    fn branch_is_merged(
        &self,
        branch_name: &str,
        base_ref: &str,
    ) -> impl Future<Output = Result<bool>> + Send;

    /// Check if a worktree path has uncommitted changes (staged or untracked).
    fn worktree_is_dirty(&self, path: &Path) -> impl Future<Output = Result<bool>> + Send;
}
```

### Pattern 2: WorktreeManager Coordinates Git + State

**What:** A `WorktreeManager` struct that owns a `GitOps` impl and manages `.smelt/worktrees/*.toml` state files.
**When to use:** All worktree lifecycle operations go through this manager.

```rust
pub struct WorktreeManager<G: GitOps> {
    git: G,
    repo_root: PathBuf,
    smelt_dir: PathBuf,   // .smelt/
}

impl<G: GitOps> WorktreeManager<G> {
    pub async fn create(&self, opts: CreateWorktreeOpts) -> Result<WorktreeInfo> {
        // 1. Validate branch name doesn't collide
        // 2. git worktree add
        // 3. Write .smelt/worktrees/<name>.toml
        // 4. Return info
    }

    pub async fn remove(&self, name: &str, force: bool) -> Result<()> {
        // 1. Load state file
        // 2. Check for uncommitted changes (prompt if dirty)
        // 3. git worktree remove
        // 4. Check branch merge status, git branch -d/-D
        // 5. Remove state file
    }

    pub async fn list(&self) -> Result<Vec<WorktreeInfo>> {
        // Cross-reference .smelt/worktrees/*.toml with git worktree list
    }

    pub async fn detect_orphans(&self) -> Result<Vec<WorktreeInfo>> {
        // 1. Read all state files
        // 2. Check PID liveness
        // 3. Cross-reference with git worktree list
    }
}
```

### Pattern 3: State File Per Worktree

**What:** One TOML file per worktree in `.smelt/worktrees/` with full session metadata.

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct WorktreeState {
    pub session_name: String,
    pub branch_name: String,
    pub worktree_path: PathBuf,
    pub base_ref: String,           // branch or commit it was based on
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub task_description: Option<String>,
    pub file_scope: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Created,
    Running,
    Completed,
    Failed,
    Orphaned,
}
```

Example TOML output:
```toml
session_name = "add-auth-flow"
branch_name = "smelt/add-auth-flow"
worktree_path = "../myrepo-smelt-add-auth-flow"
base_ref = "main"
status = "created"
created_at = "2026-03-09T14:30:00Z"
updated_at = "2026-03-09T14:30:00Z"
pid = 12345
```

### Pattern 4: Clap Subcommand with Alias

**What:** Use `#[command(visible_alias = "wt")]` for the short alias.

```rust
#[derive(Subcommand)]
enum Commands {
    Init,
    /// Manage worktrees for agent sessions
    #[command(visible_alias = "wt")]
    Worktree {
        #[command(subcommand)]
        command: WorktreeCommands,
    },
}

#[derive(Subcommand)]
enum WorktreeCommands {
    /// Create a new worktree for an agent session
    Create {
        /// Session name (used for branch and directory naming)
        name: String,
        /// Base branch or commit (defaults to HEAD)
        #[arg(long, default_value = "HEAD")]
        base: String,
        /// Custom worktree directory name
        #[arg(long)]
        dir_name: Option<String>,
    },
    /// List all tracked worktrees
    List {
        /// Show detailed output
        #[arg(short, long)]
        verbose: bool,
    },
    /// Remove a worktree and its branch
    Remove {
        /// Session name
        name: String,
        /// Force removal even with unmerged changes
        #[arg(short, long)]
        force: bool,
        /// Skip confirmation prompts
        #[arg(short, long)]
        yes: bool,
    },
    /// Clean up orphaned worktrees
    Prune {
        /// Skip confirmation prompts
        #[arg(short, long)]
        yes: bool,
    },
}
```

### Anti-Patterns to Avoid
- **Running git commands against the wrong worktree:** Always pass `-C <worktree_path>` or set `current_dir()` to the specific worktree. Never rely on the main repo's HEAD/index for worktree-specific checks.
- **Storing worktree paths as absolute in TOML:** Use relative paths (relative to repo root) so state files work if the repo is moved. Resolve to absolute at runtime.
- **Trusting PID alone for orphan detection:** PIDs can be recycled. Cross-reference with git worktree list and check if the process is actually a smelt process.
- **Deleting branches before removing worktrees:** Git will refuse to delete a branch checked out in a worktree. Always remove the worktree first, then delete the branch.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Worktree creation/removal | Custom directory + branch management | `git worktree add/remove` | Git handles `.git` file linking, HEAD/index isolation, pruning metadata |
| Porcelain output parsing | Custom `git` flag combinations | `git worktree list --porcelain` | Stable machine-parseable format: `worktree <path>\nHEAD <sha>\nbranch <ref>\n\n` |
| Branch merge detection | Custom commit graph walking | `git branch --no-merged <base>` or `git branch -d` exit code | Correct traversal of merge commits, upstream tracking |
| Dirty worktree detection | Custom file walking | `git -C <path> status --porcelain` | Handles `.gitignore`, submodules, all edge cases |
| Interactive prompts | Raw stdin/termion | `dialoguer::Confirm` | Handles terminal modes, Ctrl-C, default values, themes |
| Timestamp serialization | Manual string formatting | `chrono::DateTime<Utc>` with default serde | Serializes to RFC3339 automatically in TOML |

**Key insight:** Almost all the "hard" logic lives in git itself. Smelt's job is coordination and state tracking, not reimplementing git internals.

## Common Pitfalls

### Pitfall 1: Branch Name Collision on Create
**What goes wrong:** `git worktree add -b <name>` fails with "a branch named X already exists" if the branch already exists (even if no worktree uses it).
**Why it happens:** Git requires unique branch names across all worktrees and the main repo.
**How to avoid:** Before creating, check if branch exists with `git rev-parse --verify refs/heads/<branch>`. If it does, fail with a clear error message suggesting `--force` or a different name.
**Warning signs:** Exit code 128 from `git worktree add`.

### Pitfall 2: Worktree Path Already Exists
**What goes wrong:** `git worktree add <path>` fails if the directory already exists (even if empty).
**Why it happens:** Git won't overwrite existing directories.
**How to avoid:** Check path existence before calling git. If it exists but isn't a valid worktree, suggest cleanup.

### Pitfall 3: Removing Worktree Before Branch Deletion
**What goes wrong:** `git branch -d <branch>` fails if the branch is checked out in any worktree.
**Why it happens:** Git's safety mechanism prevents deleting active branches.
**How to avoid:** Always sequence as: (1) git worktree remove, (2) git branch -d/-D. Never reverse this order.
**Warning signs:** "error: Cannot delete branch 'X' checked out at 'Y'"

### Pitfall 4: State File Desync
**What goes wrong:** `.smelt/worktrees/*.toml` says a worktree exists, but `git worktree list` disagrees (or vice versa).
**Why it happens:** Manual `git worktree remove`, filesystem deletion, or Smelt crash mid-operation.
**How to avoid:** Always cross-reference both sources during `list` and `prune`. Report discrepancies to the user. Treat git as source of truth for "does the worktree physically exist?" and Smelt state as source of truth for "what session metadata exists?".
**Warning signs:** State file exists but worktree path is gone, or git lists a worktree with no corresponding state file.

### Pitfall 5: PID Recycling in Orphan Detection
**What goes wrong:** A PID from a dead Smelt session gets recycled by the OS for an unrelated process.
**Why it happens:** PIDs are finite and recycled on all POSIX systems.
**How to avoid:** Don't rely solely on `kill(pid, 0)`. Also check:
  1. Does the worktree path still exist?
  2. Does `git worktree list` include it?
  3. Has the state file's `updated_at` timestamp gone stale (no updates for a configurable threshold)?
**Warning signs:** Orphan detection says "process alive" but the session hasn't updated state in hours.

### Pitfall 6: Relative vs Absolute Paths
**What goes wrong:** Worktree paths break when commands are run from different working directories.
**Why it happens:** `git worktree add ../sibling` creates a relative reference, but the worktree path is stored absolutely by git.
**How to avoid:** Always canonicalize paths before storage. Use `std::fs::canonicalize()` or `std::path::absolute()` (stabilized in Rust 1.79). Store paths relative to repo root in state files, resolve to absolute at runtime.

## Code Examples

### Parsing `git worktree list --porcelain`

Verified porcelain format from testing:
```
worktree /absolute/path/to/worktree
HEAD <40-char-sha>
branch refs/heads/<branch-name>
<blank line>
```

```rust
/// Entry from `git worktree list --porcelain`.
#[derive(Debug)]
pub struct GitWorktreeEntry {
    pub path: PathBuf,
    pub head: String,
    pub branch: Option<String>,  // None for detached HEAD
    pub is_bare: bool,
    pub is_locked: bool,
}

fn parse_porcelain(output: &str) -> Vec<GitWorktreeEntry> {
    let mut entries = Vec::new();
    let mut path = None;
    let mut head = None;
    let mut branch = None;
    let mut is_bare = false;

    for line in output.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            // If we have a previous entry, push it
            if let (Some(p), Some(h)) = (path.take(), head.take()) {
                entries.push(GitWorktreeEntry {
                    path: PathBuf::from(p),
                    head: h,
                    branch: branch.take(),
                    is_bare,
                    is_locked: false,
                });
                is_bare = false;
            }
            path = Some(p.to_string());
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            head = Some(h.to_string());
        } else if let Some(b) = line.strip_prefix("branch ") {
            // Strip refs/heads/ prefix
            branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
        } else if line == "bare" {
            is_bare = true;
        } else if line.is_empty() {
            // Entry separator - push current entry
            if let (Some(p), Some(h)) = (path.take(), head.take()) {
                entries.push(GitWorktreeEntry {
                    path: PathBuf::from(p),
                    head: h,
                    branch: branch.take(),
                    is_bare,
                    is_locked: false,
                });
                is_bare = false;
            }
        }
    }
    // Don't forget last entry if no trailing newline
    if let (Some(p), Some(h)) = (path.take(), head.take()) {
        entries.push(GitWorktreeEntry {
            path: PathBuf::from(p),
            head: h,
            branch: branch.take(),
            is_bare,
            is_locked: false,
        });
    }
    entries
}
```

### PID Liveness Check (POSIX)

```rust
/// Check if a process with the given PID is alive.
/// Returns `true` if the process exists and we have permission to signal it.
fn is_pid_alive(pid: u32) -> bool {
    // SAFETY: kill(pid, 0) sends no signal, only checks if the process exists.
    // This is a standard POSIX pattern.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}
```

Note: This requires `libc` as a dependency. It's already a transitive dependency (via tokio, chrono, etc.) but should be added explicitly to `smelt-core`'s Cargo.toml.

### Confirmation Prompt Pattern

```rust
use dialoguer::Confirm;

fn confirm_destructive(message: &str, skip_confirm: bool) -> Result<bool> {
    if skip_confirm {
        return Ok(true);
    }
    Confirm::new()
        .with_prompt(message)
        .default(false)
        .interact()
        .map_err(|e| SmeltError::io("reading confirmation", "stdin", e.into()))
}
```

### Worktree Naming Convention

Recommended branch prefix: `smelt/` — clear namespace, avoids collision with user branches.

```rust
fn worktree_branch_name(session_name: &str) -> String {
    format!("smelt/{session_name}")
}

fn worktree_directory_name(repo_name: &str, session_name: &str) -> String {
    format!("{repo_name}-smelt-{session_name}")
}
```

Example: repo `myproject`, session `add-auth` produces:
- Branch: `smelt/add-auth`
- Directory: `../myproject-smelt-add-auth/`

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|-------------|-----------------|--------------|--------|
| `git worktree list` text parsing | `git worktree list --porcelain` | Git 2.7+ (2016) | Machine-parseable, stable format |
| Manual worktree cleanup | `git worktree prune` | Git 2.5+ (2015) | Handles stale admin files automatically |
| `async-trait` crate for trait methods | Native `async fn` in traits (RPITIT) | Rust 1.75 (2023) | No crate dependency, cleaner signatures |
| `std::path::absolute()` unstable | Stabilized in Rust 1.79 | Rust 1.79 (2024) | No need for external path canonicalization crate |

**Deprecated/outdated:**
- `chrono::Utc.timestamp()` constructor: Use `DateTime::from_timestamp_secs()` instead (changed in chrono 0.4.35+)
- `dialoguer` 0.10.x: Current is 0.12.x with breaking API changes (method names, theme system)

## Open Questions

1. **Worktree path storage: relative or absolute?**
   - What we know: Git stores absolute paths internally. State files committed to git should use relative paths for portability.
   - What's unclear: If sibling directories (`../repo-smelt-session/`) should be stored as relative to repo root or as absolute.
   - Recommendation: Store relative to repo root in TOML. Resolve at runtime. Use `std::path::absolute()` for display.

2. **Orphan detection timing threshold**
   - What we know: PID checking alone is insufficient due to PID recycling.
   - What's unclear: What's a reasonable "stale" threshold for `updated_at` — 1 hour? 24 hours? Configurable?
   - Recommendation: Start with a conservative 24-hour threshold. Make configurable in `.smelt/config.toml` later if needed.

3. **State file locking during concurrent operations**
   - What we know: Multiple smelt processes could theoretically write to the same state file.
   - What's unclear: Whether file-level locking (e.g., `fs2::FileExt`) is needed for v0.1.
   - Recommendation: Defer locking. v0.1 is single-user. Document as a known limitation.

## New Error Variants Needed

The existing `SmeltError` enum needs these additions:

```rust
pub enum SmeltError {
    // ... existing variants ...

    /// Worktree with this name already exists.
    #[error("worktree '{name}' already exists")]
    WorktreeExists { name: String },

    /// Worktree not found in Smelt state.
    #[error("worktree '{name}' not found")]
    WorktreeNotFound { name: String },

    /// Branch already exists (collision).
    #[error("branch '{branch}' already exists")]
    BranchExists { branch: String },

    /// Worktree has uncommitted changes.
    #[error("worktree '{name}' has uncommitted changes (use --force to override)")]
    WorktreeDirty { name: String },

    /// Branch has unmerged commits.
    #[error("branch '{branch}' has unmerged commits (use --force to delete)")]
    BranchUnmerged { branch: String },

    /// Smelt project not initialized.
    #[error("not a Smelt project (run `smelt init` first)")]
    NotInitialized,

    /// TOML deserialization error.
    #[error("failed to parse state file: {0}")]
    StateDeserialization(#[from] toml::de::Error),
}
```

## Sources

### Primary (HIGH confidence)
- Git worktree CLI — tested directly against git 2.x on macOS; porcelain format, branch collision, remove behavior, dirty worktree detection all verified empirically
- clap `/websites/rs_clap` (Context7) — `visible_alias` for subcommand aliasing, derive API for nested subcommands
- chrono `/websites/rs_chrono_chrono` (Context7) — `DateTime<Utc>` default serde serialization to RFC3339, feature flags
- Existing codebase — Phase 1 `GitOps` trait, `GitCli` impl, `SmeltError` enum reviewed

### Secondary (MEDIUM confidence)
- dialoguer 0.12 — [docs.rs/dialoguer](https://docs.rs/dialoguer/latest/dialoguer/) — `Confirm` API with `default()`, `interact()`, `interact_opt()`
- opencode worktree orphan cleanup — [PR #14649](https://github.com/anomalyco/opencode/pull/14649) — cleanup sequence: directory, worktree entry, branch, state record; best-effort approach

### Tertiary (LOW confidence)
- PID liveness via `libc::kill(pid, 0)` — standard POSIX pattern, well-documented, but PID recycling caveat requires additional signals

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all crates verified via Context7/docs.rs, versions confirmed
- Architecture: HIGH — patterns derived from existing Phase 1 codebase and git CLI behavior verified empirically
- Pitfalls: HIGH — all pitfalls verified through direct git worktree testing (branch collision, dirty removal, delete ordering)
- State management: MEDIUM — TOML schema is straightforward but concurrent access patterns untested

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable domain, git worktree API hasn't changed in years)
