# Phase 4: Sequential Merge - Research

**Researched:** 2026-03-09
**Confidence levels:** HIGH = verified with experiments, MEDIUM = docs + reasoning, LOW = uncertain/unverified

---

## Standard Stack

No new external crates required. All merge operations use `git` CLI shell-out via the existing `GitOps` trait pattern.

| Concern | Solution | Status |
|---------|----------|--------|
| Squash merge | `git merge --squash <branch>` | Existing `git` CLI |
| Merge-base detection | `git merge-base <a> <b>` | Existing `git` CLI |
| Branch creation | `git branch <name> <start-point>` | Existing `git` CLI |
| Diff stats | `git diff --numstat <a> <b>` | Existing `git` CLI |
| Conflict detection | Exit code + `git status --porcelain` | Existing `git` CLI |
| LLM commit messages | Shell out to `claude` CLI or `reqwest` + Anthropic API | See LLM section |

**Confidence: HIGH** — All git operations verified experimentally.

---

## Architecture Patterns

### Merge Worktree Pattern

**Critical finding:** `git merge --squash` requires being checked out on the target branch. You cannot squash-merge into a branch you are not on. Since the main repo worktree is occupied by `main` (or whatever the user's branch is), the merge must happen inside a **temporary worktree** for the target branch.

**Verified sequence:**

```
1. git branch smelt/merge/<manifest-name> <base-commit>    # Create target branch at common ancestor
2. git worktree add <temp-path> smelt/merge/<manifest-name> # Check out target in temp worktree
3. (in temp worktree) git merge --squash <session-branch>   # Squash merge each session
4. (in temp worktree) git commit -m "<message>"             # Commit each squash
5. git worktree remove <temp-path>                          # Clean up temp worktree
```

This pattern was experimentally verified: merges in the temp worktree correctly update the target branch as visible from the main repo.

**Confidence: HIGH**

### MergeRunner Structure

Follow the `SessionRunner<G: GitOps>` pattern:

```rust
pub struct MergeRunner<G: GitOps> {
    git: G,
    repo_root: PathBuf,
}
```

- Takes a `Manifest` and produces a `MergeReport`
- Reads `.smelt/worktrees/*.toml` to find completed sessions
- Creates the target branch + temp worktree
- Iterates sessions, squash-merging each
- On success: cleans up session branches
- On failure: deletes target branch, preserves session worktrees

**Confidence: HIGH**

### Module Layout

```
smelt-core/src/merge/
  mod.rs       — MergeRunner, MergeReport, public API
  types.rs     — MergeSessionResult, DiffStat, MergeError types
```

CLI addition:
```
smelt-cli/src/commands/merge.rs — `smelt merge <manifest.toml>` handler
```

**Confidence: HIGH**

---

## Git Operations — Detailed Mechanics

### 1. Squash Merge (MERGE-01 core)

**Command:** `git merge --squash <branch>`

**Behavior:**
- Stages all changes from `<branch>` into the index (working tree + index updated)
- Does NOT create a commit (unlike regular merge)
- Does NOT set MERGE_HEAD (unlike regular merge — important for abort behavior)
- Exit code 0 on success, 1 on conflict

**After squash, you must explicitly commit:**
`git commit -m "<message>"`

**Confidence: HIGH** — Verified experimentally.

### 2. Merge-Base Detection

**Command:** `git merge-base <commit-a> <commit-b>`

**Behavior:** Returns the SHA of the best common ancestor between two commits.

**Strategy for Smelt:** All session branches fork from the same base (the manifest's `base_ref` resolved at worktree creation time). The common ancestor can be found via:

- **Option A (preferred):** `git merge-base <session-branch-1> <session-branch-2>` — works when all sessions share the same fork point
- **Option B:** Resolve stored `base_ref` from any session's `WorktreeState` — but `base_ref` stores the symbolic ref (e.g., "HEAD", "main"), not the commit SHA. This is fragile if the base branch has moved.
- **Option C:** Store the resolved commit SHA at worktree creation time — requires adding a field to `WorktreeState`

**Recommendation:** Use Option A for this phase. Take `merge-base` of the first two completed session branches. If only one session, use `git merge-base <session-branch> <current-base-ref>`. This is robust and requires no schema changes.

**Confidence: HIGH** — Verified experimentally. When sessions branch from the same point, `merge-base` correctly returns that common ancestor.

### 3. Conflict Detection

**On conflict, `git merge --squash` returns exit code 1** and writes to stderr:
```
Auto-merging <file>
CONFLICT (content): Merge conflict in <file>
Automatic merge failed; fix conflicts and then commit the result.
```

**Parsing conflicting files:** Use `git status --porcelain` after a failed merge:
```
UU shared.txt    # Both modified (content conflict)
AA file.txt      # Both added
DD file.txt      # Both deleted
```

Files with `UU`, `AA`, `DD`, `AU`, `UA`, `DU`, `UD` prefixes are unmerged. Parse with: any line where both columns are from the set `{U, A, D}` and neither is a space.

**Simpler approach:** `git diff --name-only --diff-filter=U` lists only unmerged files.

**Confidence: HIGH** — Verified experimentally.

### 4. Rollback After Conflict

**Critical finding:** `git merge --abort` does NOT work after `git merge --squash` because MERGE_HEAD is not set. Git returns:
```
fatal: There is no merge to abort (MERGE_HEAD missing).
```

**Correct rollback sequence:**
```
1. git reset --hard HEAD          # Undo the failed squash merge in the temp worktree
2. git worktree remove <temp>     # Remove the temp worktree
3. git branch -D <target-branch>  # Delete the target branch entirely
```

Since we're working in a temporary worktree, rollback is straightforward: just destroy the worktree and the branch.

**Confidence: HIGH** — Verified experimentally.

### 5. Diff Stat Generation

**Command:** `git diff --numstat <before-commit> <after-commit>`

**Output format (machine-parseable):**
```
1	0	a.txt
1	1	file.txt
1	0	new.txt
```

Columns: `<insertions>\t<deletions>\t<filename>`

**For per-session stats:** After each squash commit, capture `HEAD~1` and `HEAD`, then run `git diff --numstat HEAD~1 HEAD`.

**Alternative:** `git diff --shortstat HEAD~1 HEAD` gives a one-line summary:
```
 3 files changed, 3 insertions(+), 1 deletion(-)
```

**Recommendation:** Use `--numstat` for structured parsing, format into a summary line per session. Use `--shortstat` as a shortcut if only the summary is needed.

**Confidence: HIGH** — Verified experimentally.

### 6. Branch Creation at Specific Commit

**Command:** `git branch <name> <start-point>`

Creates the branch without checking it out. Then `git worktree add <path> <branch>` checks it out in the temp worktree.

**Confidence: HIGH**

---

## GitOps Trait Extensions

New methods needed on the `GitOps` trait:

```rust
/// Find the best common ancestor between two commits.
fn merge_base(
    &self,
    commit_a: &str,
    commit_b: &str,
) -> impl Future<Output = Result<String>> + Send;

/// Squash-merge a branch into the current branch in `work_dir`.
/// Returns Ok(()) on success, Err with conflict info on failure.
fn merge_squash(
    &self,
    work_dir: &Path,
    branch: &str,
) -> impl Future<Output = Result<()>> + Send;

/// Create a branch at a specific start point (without checking it out).
fn branch_create(
    &self,
    branch_name: &str,
    start_point: &str,
) -> impl Future<Output = Result<()>> + Send;

/// Get diff stats between two commits. Returns Vec<(insertions, deletions, filename)>.
fn diff_numstat(
    &self,
    from: &str,
    to: &str,
) -> impl Future<Output = Result<Vec<(usize, usize, String)>>> + Send;

/// Get the short stat summary between two commits.
fn diff_shortstat(
    &self,
    from: &str,
    to: &str,
) -> impl Future<Output = Result<String>> + Send;

/// Get the list of unmerged (conflicting) files in a working directory.
fn unmerged_files(
    &self,
    work_dir: &Path,
) -> impl Future<Output = Result<Vec<String>>> + Send;

/// Hard reset the working directory to HEAD (used for conflict rollback).
fn reset_hard(
    &self,
    work_dir: &Path,
) -> impl Future<Output = Result<()>> + Send;

/// Resolve a ref to its full commit SHA.
fn rev_parse(
    &self,
    rev: &str,
) -> impl Future<Output = Result<String>> + Send;
```

**Note on `merge_squash`:** The `run_in` method in `GitCli` currently treats any non-zero exit as an error via `SmeltError::GitExecution`. For `merge --squash`, exit code 1 means "conflict" which is a distinct error path, not a general git failure. Implementation options:

1. **Add a `run_in_raw` method** that returns `(exit_code, stdout, stderr)` without error-mapping, used only for merge operations
2. **Parse the `GitExecution` error** downstream — fragile, not recommended
3. **Custom method** that handles the exit code internally

**Recommendation:** Add a private `run_in_raw` method to `GitCli` that returns the raw output. `merge_squash` uses it to distinguish exit 0 (success) from exit 1 (conflict) from other codes (actual error). This keeps the public `run_in` unchanged.

**Confidence: HIGH**

---

## Don't Hand-Roll

| Problem | Use Instead |
|---------|-------------|
| Squash merge implementation | `git merge --squash` — never manually cherry-pick or patch |
| Merge-base calculation | `git merge-base` — never walk commit graph manually |
| Diff stat calculation | `git diff --numstat` — never count lines manually |
| Conflict detection | `git status --porcelain` or `git diff --name-only --diff-filter=U` |
| Branch creation at commit | `git branch <name> <point>` — never manually write refs |
| Temp worktree for merge | `git worktree add/remove` — never checkout in the main worktree |

---

## LLM Commit Message Generation

### Context

Each squash merge commit needs a descriptive message. The CONTEXT.md specifies "LLM-generated commit messages from session metadata (session name, task description)."

### Options Evaluated

| Approach | Pros | Cons |
|----------|------|------|
| **A: Shell out to `claude` CLI** | No new deps, uses user's existing auth, follows project's shell-out pattern | Requires `claude` CLI installed, slower startup per call |
| **B: `reqwest` + raw Anthropic API** | Direct control, async-native | New dep (reqwest), API key management, HTTP plumbing |
| **C: Community Rust crate** | Less boilerplate than B | No official SDK, unstable third-party crates, still need API key |
| **D: Template-based (no LLM)** | Zero external deps, deterministic, fast, always works | Less descriptive messages |

### Recommendation: Hybrid D+A (Template default, optional LLM)

**Default: Template-based commit messages (no LLM required).**

Format:
```
merge(<session-name>): <task-description-truncated-to-72-chars>

Squash merge of session '<session-name>' into <target-branch>.
```

This ensures `smelt merge` works out of the box without any LLM dependency. The merge is a git operations tool, not an AI tool — it should not require an API key to function.

**Optional: LLM-enhanced messages via `--llm-messages` flag (Phase 5+ or defer).**

If implemented, shell out to `claude` CLI (Option A) since:
- Smelt already uses the shell-out pattern for `git`
- Users of Smelt likely already have `claude` CLI (they're running AI agents)
- No new Rust deps needed
- Auth handled by the CLI's existing config

**For Phase 4, implement only the template-based approach.** LLM messages can be added as a flag later without changing the merge architecture.

**Confidence: MEDIUM** — The template approach is definitively correct for Phase 4. The LLM approach is a design recommendation for future phases.

---

## Common Pitfalls

### 1. `git merge --abort` does not work after `--squash`
MERGE_HEAD is not set during squash merge. Use `git reset --hard HEAD` instead.
**Verified:** YES — experimentally confirmed.

### 2. Squash merge requires checkout on target branch
Cannot squash-merge into a branch you're not on. Must use a temporary worktree.
**Verified:** YES — experimentally confirmed.

### 3. `base_ref` in WorktreeState is symbolic, not a commit SHA
The stored `base_ref` might be "HEAD" or "main" which could resolve to different commits over time. Use `git merge-base` between session branches instead of resolving the stored ref.
**Verified:** YES — code inspection + reasoning.

### 4. Worktree path for merge temp must not collide
The temporary worktree for merging needs a unique path. Use `<repo-parent>/<repo-name>-smelt-merge-<manifest-name>` to match existing naming conventions.

### 5. Cleanup order matters
On success: (1) remove temp merge worktree, (2) delete session branches, (3) remove session worktrees. On failure: (1) reset hard in temp worktree, (2) remove temp worktree, (3) delete target branch. Never delete session worktrees on failure.

### 6. Session branch names follow `smelt/<session-name>` convention
The merge code must read this from `WorktreeState.branch_name`, not construct it manually.

### 7. Empty session list (all failed/incomplete) must error early
Check for at least 1 completed session before creating the target branch. Otherwise you'd create and immediately delete an empty branch.

### 8. Race condition: session worktrees still in use
The merge command should verify no sessions are in `Running` status before proceeding. A session that's still running could have its branch modified mid-merge.

---

## Code Examples

### Reading completed sessions from state files

```rust
fn load_completed_sessions(smelt_dir: &Path) -> Result<Vec<WorktreeState>> {
    let worktrees_dir = smelt_dir.join("worktrees");
    let mut completed = Vec::new();

    for entry in std::fs::read_dir(&worktrees_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let state = WorktreeState::load(&path)?;
        match state.status {
            SessionStatus::Completed => completed.push(state),
            SessionStatus::Running => {
                // Warn: session still running
                eprintln!("warning: session '{}' is still running, skipping", state.session_name);
            }
            _ => {
                eprintln!("warning: session '{}' status {:?}, skipping", state.session_name, state.status);
            }
        }
    }

    Ok(completed)
}
```

### Squash merge with conflict detection (GitCli)

```rust
async fn merge_squash(&self, work_dir: &Path, branch: &str) -> Result<()> {
    let output = Command::new(&self.git_binary)
        .args(["merge", "--squash", branch])
        .current_dir(work_dir)
        .output()
        .await
        .map_err(|e| SmeltError::io("running git merge --squash", work_dir, e))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("CONFLICT") {
        // Parse conflicting files
        let files = self.unmerged_files(work_dir).await?;
        return Err(SmeltError::MergeConflict {
            session: branch.to_string(),
            files,
        });
    }

    Err(SmeltError::GitExecution {
        operation: format!("merge --squash {branch}"),
        message: stderr.trim().to_string(),
    })
}
```

### Parsing `--numstat` output

```rust
fn parse_numstat(output: &str) -> Vec<(usize, usize, String)> {
    output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() == 3 {
                let ins = parts[0].parse().unwrap_or(0);
                let del = parts[1].parse().unwrap_or(0);
                Some((ins, del, parts[2].to_string()))
            } else {
                None
            }
        })
        .collect()
}
```

### Template commit message

```rust
fn format_commit_message(session_name: &str, task: Option<&str>, target_branch: &str) -> String {
    let subject = match task {
        Some(desc) => {
            let prefix = format!("merge({session_name}): ");
            let max_desc = 72 - prefix.len();
            if desc.len() > max_desc {
                format!("{prefix}{}...", &desc[..max_desc - 3])
            } else {
                format!("{prefix}{desc}")
            }
        }
        None => format!("merge({session_name}): squash merge into {target_branch}"),
    };

    format!(
        "{subject}\n\nSquash merge of session '{session_name}' into {target_branch}."
    )
}
```

---

## New Error Variants

Add to `SmeltError`:

```rust
/// Merge conflict detected during squash merge.
#[error("merge conflict in session '{session}': conflicting files: {}", files.join(", "))]
MergeConflict { session: String, files: Vec<String> },

/// Target merge branch already exists.
#[error("merge target branch '{branch}' already exists (use --force or delete it)")]
MergeTargetExists { branch: String },

/// No completed sessions available to merge.
#[error("no completed sessions found to merge")]
NoCompletedSessions,
```

---

## Open Questions (for planner to resolve)

1. **Session ordering:** The CONTEXT says "sessions merged sequentially" but doesn't specify order. Use manifest order (order they appear in TOML `[[session]]` array). This is deterministic and matches user intent.

2. **Worktree cleanup scope:** CONTEXT says "session worktree branches auto-cleaned after full merge sequence succeeds." Does this mean delete the worktree directories too, or just the branches? Recommend: delete both (worktree dir + branch + state file) using existing `WorktreeManager::remove(name, force=true)`.

3. **Temp worktree naming:** Suggest `<repo-name>-smelt-merge-<manifest-name>` in the same parent directory as session worktrees.

---

*Phase: 04-sequential-merge*
*Research completed: 2026-03-09*
