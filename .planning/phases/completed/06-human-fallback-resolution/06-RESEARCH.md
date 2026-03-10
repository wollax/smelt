# Phase 6: Human Fallback Resolution — Research

**Date:** 2026-03-10
**Mode:** Ecosystem / Implementation patterns
**Confidence key:** HIGH = verified via docs + tested | MEDIUM = verified via one source | LOW = training data only

---

## Standard Stack

All libraries are already workspace dependencies. No new crates required.

| Library | Version | Role in Phase 6 |
|---------|---------|-----------------|
| `dialoguer` | 0.12 | `Select` prompt for resolve/skip/abort menu |
| `console` | 0.16 | `Term::stderr()` for `read_key()`, `Style` for colored conflict output |
| `tokio` | 1.x | Async runtime — `spawn_blocking` for synchronous dialoguer/console calls |
| `tracing` | 0.1 | Structured logging for merge progress events |

**Confidence: HIGH** — all verified in workspace Cargo.toml and Cargo.lock.

---

## Architecture Patterns

### Pattern 1: Conflict Handler Callback (recommended)

The current `merge_sessions()` loop calls `merge_squash()` and on `MergeConflict` error, propagates it upward causing full rollback. Phase 6 needs to intercept that error and enter a resolution flow.

**Recommended approach:** Introduce a `ConflictHandler` trait that `MergeRunner` accepts, keeping interactive logic out of `smelt-core`.

```rust
/// Defined in smelt-core. Determines what happens when a merge conflict occurs.
pub enum ConflictAction {
    /// User resolved the conflict — files are already edited on disk.
    Resolved,
    /// Skip this session entirely.
    Skip,
    /// Abort the entire merge sequence.
    Abort,
}

/// Trait for handling merge conflicts. Implemented in smelt-cli for interactive prompts.
pub trait ConflictHandler: Send + Sync {
    fn handle_conflict(
        &self,
        session_name: &str,
        conflicted_files: &[String],
        worktree_path: &Path,
    ) -> impl Future<Output = ConflictAction> + Send;
}
```

The CLI crate implements this with dialoguer prompts. Tests can implement it with a mock that always returns `Skip` or `Abort`. This preserves the `smelt-core` / `smelt-cli` separation established in prior phases.

**Confidence: HIGH** — follows the same seam pattern as `GitOps` trait already in the codebase.

### Pattern 2: Resolution Loop in merge_sessions()

The `merge_sessions()` method changes from propagating `MergeConflict` to calling the conflict handler:

```
for session in sessions:
    match merge_squash():
        Ok(()) => commit and continue
        Err(MergeConflict) => match handler.handle_conflict():
            Resolved => validate markers, git add, commit (loop on re-prompt)
            Skip => git reset --hard HEAD, record as skipped
            Abort => return Err (caller handles keep/rollback)
```

**Confidence: HIGH** — tested the git commands (`reset --hard`, `add`, `diff --check`) in a real conflict scenario; all behave as expected.

### Pattern 3: Interactive Prompt in CLI Layer

`dialoguer::Select` returns the index of the selected item. Map indices to actions:

```rust
let items = &["[r]esolve — edit files externally, then validate",
               "[s]kip — discard this session's changes",
               "[a]bort — stop the merge"];
let selection = Select::with_theme(&ColorfulTheme::default())
    .with_prompt("Conflict in session 'foo'")
    .items(items)
    .default(0)
    .interact_on(&Term::stderr())?;
match selection {
    0 => ConflictAction::Resolved,
    1 => ConflictAction::Skip,
    _ => ConflictAction::Abort,
}
```

dialoguer `Select` uses arrow keys + Enter (not single-character shortcuts). The `[r]`/`[s]`/`[a]` prefixes are visual hints only — navigation is standard up/down/enter.

**Confidence: HIGH** — verified via docs.rs for dialoguer 0.12.

### Pattern 4: Validation Loop (re-prompt on unresolved markers)

After user presses Enter to signal "resolved":
1. Read each conflicted file, scan for conflict markers
2. If markers remain, print which files still have markers, re-prompt
3. If clean, `git add` the files and commit

```rust
loop {
    // Wait for user to signal ready
    term.write_line("Edit the conflicted files externally, then press Enter...")?;
    term.read_key()?; // blocks until keypress

    let remaining = check_conflict_markers(worktree_path, &conflicted_files)?;
    if remaining.is_empty() {
        break; // All resolved
    }
    term.write_line(&format!(
        "{} file(s) still have conflict markers:", remaining.len()
    ))?;
    for f in &remaining {
        term.write_line(&format!("  {}", f))?;
    }
}
```

Use `Term::stderr()` for all interactive output (consistent with existing tracing and eprintln patterns in the codebase).

**Confidence: HIGH** — `Term::read_key()` verified to block without echo, `Key::Enter` and `Key::Char(_)` both work.

---

## Don't Hand-Roll

| Problem | Use Instead | Why |
|---------|-------------|-----|
| Interactive menu selection | `dialoguer::Select` | Already a dependency; handles terminal raw mode, arrow keys, rendering |
| Terminal styling (bold, color) | `console::Style` | Already a dependency via dialoguer; auto-detects TTY vs pipe |
| Single keypress reading | `console::Term::read_key()` | Handles raw mode correctly on macOS/Linux; no echo |
| Conflict marker detection in files | Line-by-line scan with `starts_with` | Do NOT use regex — conflict markers are fixed strings at line start |
| Listing unmerged files | `git diff --name-only --diff-filter=U` | Already implemented as `GitOps::unmerged_files()` |
| Undoing a failed squash merge | `git reset --hard HEAD` | Already implemented as `GitOps::reset_hard()` |
| Staging resolved files | `git add <files>` | Already implemented as `GitOps::add()` |

---

## Common Pitfalls

### Pitfall 1: Blocking dialoguer in async context
**Problem:** dialoguer's `Select::interact()` and `console::Term::read_key()` are synchronous blocking calls. Calling them directly inside an async function will block the tokio runtime thread.

**Prevention:** Wrap all interactive calls in `tokio::task::spawn_blocking()`:
```rust
let action = tokio::task::spawn_blocking(move || {
    Select::new().items(&items).interact()
}).await.unwrap()?;
```

**Confidence: HIGH** — standard tokio pattern; existing dialoguer calls in worktree.rs are in non-async CLI handler functions (called from async but the prompt itself runs in the main thread context via `.await`). The new conflict handler will be called deeper in the async stack, requiring explicit `spawn_blocking`.

### Pitfall 2: Conflict markers that look like content
**Problem:** A source file might legitimately contain `<<<<<<<` as content (e.g., in documentation or test files).

**Prevention:** Only scan files that `git diff --name-only --diff-filter=U` reports as unmerged. After resolution and `git add`, git itself tracks the file as resolved. The marker scan is a UX safety check, not the authoritative state.

**Confidence: HIGH** — `--diff-filter=U` reliably lists only unmerged paths (verified experimentally).

### Pitfall 3: git diff --check exit codes
**Problem:** `git diff --check` exits with code 2 when conflict markers are found, not code 1. Code 1 means "differences found" (no conflict markers).

**Prevention:** Do NOT use `git diff --check` for programmatic conflict marker detection. Instead, read the files directly and scan for marker lines. The `run_in()` helper treats all non-zero exits as errors.

**Recommendation:** Use direct file reading for marker detection — it's simpler and avoids exit code ambiguity:
```rust
fn has_conflict_markers(path: &Path) -> io::Result<bool> {
    let content = std::fs::read_to_string(path)?;
    Ok(content.lines().any(|line| {
        line.starts_with("<<<<<<<") || line.starts_with("=======") || line.starts_with(">>>>>>>")
    }))
```

**Confidence: HIGH** — exit code behavior verified experimentally (`git diff --check` returned exit 2 on conflict markers).

### Pitfall 4: Squash merge leaves staged AND unstaged changes on conflict
**Problem:** After `git merge --squash` with conflicts, git leaves the working tree in a mixed state: non-conflicting changes are staged, conflicting files appear as both staged and unmerged (porcelain status `UU`).

**Prevention:** On skip, `git reset --hard HEAD` cleanly undoes everything (both staged and unstaged). On resolve, only `git add` the conflicted files (non-conflicting files are already staged from the squash merge).

**Confidence: HIGH** — verified: `git status --porcelain` showed `UU file.txt` during conflict, and `git reset --hard HEAD` cleanly restored state.

### Pitfall 5: Console output interleaving with tracing
**Problem:** tracing writes to stderr. dialoguer and console also write to stderr. If tracing emits a log line during an interactive prompt, the display corrupts.

**Prevention:** Use `Term::stderr()` consistently for all interactive output. dialoguer's `interact_on(&Term::stderr())` is explicit about which terminal handle it uses. Tracing output at `info` level is fine since it happens before/after prompts, not during.

**Confidence: MEDIUM** — tracing subscriber is configured with `stderr` writer in main.rs; dialoguer prompts block the thread so interleaving shouldn't occur in practice.

### Pitfall 6: `=======` false positives in conflict marker detection
**Problem:** The `=======` separator (exactly 7 `=` chars) might appear in legitimate content more often than `<<<<<<<` or `>>>>>>>`.

**Prevention:** Only flag a file as having conflict markers if it contains ALL THREE marker types (`<<<<<<<`, `=======`, `>>>>>>>`). A single `=======` without opening/closing markers is not a conflict.

**Confidence: HIGH** — this is standard practice in git tools.

---

## Code Examples

### Example 1: Conflict Marker Detection (file-level)

```rust
use std::path::Path;

/// Result of scanning a file for git conflict markers.
pub struct ConflictScan {
    pub has_markers: bool,
    pub hunks: Vec<ConflictHunk>,
    pub total_conflict_lines: usize,
}

pub struct ConflictHunk {
    pub start_line: usize,  // line number of <<<<<<<
    pub end_line: usize,    // line number of >>>>>>>
}

/// Scan a file for git conflict markers. Returns hunk locations and line counts.
pub fn scan_conflict_markers(path: &Path) -> std::io::Result<ConflictScan> {
    let content = std::fs::read_to_string(path)?;
    let mut hunks = Vec::new();
    let mut current_start: Option<usize> = None;
    let mut has_separator = false;

    for (i, line) in content.lines().enumerate() {
        let line_num = i + 1; // 1-indexed
        if line.starts_with("<<<<<<<") {
            current_start = Some(line_num);
            has_separator = false;
        } else if line.starts_with("=======") && current_start.is_some() {
            has_separator = true;
        } else if line.starts_with(">>>>>>>") && current_start.is_some() && has_separator {
            hunks.push(ConflictHunk {
                start_line: current_start.unwrap(),
                end_line: line_num,
            });
            current_start = None;
            has_separator = false;
        }
    }

    let total_conflict_lines: usize = hunks.iter().map(|h| h.end_line - h.start_line + 1).sum();
    Ok(ConflictScan {
        has_markers: !hunks.is_empty(),
        hunks,
        total_conflict_lines,
    })
}
```

**Key detail:** Requires all three markers in sequence (`<<<<<<<` then `=======` then `>>>>>>>`) to count as a conflict hunk. A stray `=======` alone is not a conflict.

### Example 2: Conflict Summary Display

```rust
use console::Style;

fn display_conflict_summary(
    term: &console::Term,
    session_name: &str,
    files: &[(String, ConflictScan)],  // (path, scan result)
    index: usize,
    total: usize,
) -> std::io::Result<()> {
    let bold = Style::new().bold();
    let dim = Style::new().dim();
    let red = Style::new().red().bold();

    term.write_line(&format!(
        "\n{} Conflict in session '{}' [{}/{}]",
        red.apply_to("CONFLICT"),
        bold.apply_to(session_name),
        index + 1,
        total,
    ))?;

    for (path, scan) in files {
        let hunk_ranges: Vec<String> = scan.hunks.iter()
            .map(|h| format!("L{}-L{}", h.start_line, h.end_line))
            .collect();
        term.write_line(&format!(
            "  {} {} ({})",
            red.apply_to("C"),
            path,
            dim.apply_to(hunk_ranges.join(", ")),
        ))?;

        // Inline display for small conflicts (<20 lines total in this file)
        if scan.total_conflict_lines < 20 {
            // Read and display the conflict region
            display_inline_conflict(term, path, scan)?;
        } else {
            term.write_line(&format!(
                "    {} ({} conflict lines — edit externally)",
                dim.apply_to("..."),
                scan.total_conflict_lines,
            ))?;
        }
    }
    Ok(())
}
```

### Example 3: Resume Detection via git log

```rust
/// Detect which sessions are already merged into the target branch.
/// Parses commit subjects matching `merge(<session>):` pattern.
async fn detect_merged_sessions(
    git: &impl GitOps,
    target_branch: &str,
    base_ref: &str,
) -> Result<HashSet<String>> {
    // git log --oneline --format="%s" base..target
    let range = format!("{base_ref}..{target_branch}");
    let output = git.log_oneline_subjects(&range).await?;

    let mut merged = HashSet::new();
    for line in output.lines() {
        // Parse: "merge(session-name): ..."
        if let Some(rest) = line.strip_prefix("merge(") {
            if let Some(idx) = rest.find(')') {
                merged.insert(rest[..idx].to_string());
            }
        }
    }
    Ok(merged)
}
```

This requires a new `GitOps` method: `log_oneline_subjects(range)` that runs `git log --format="%s" <range>`. Simple addition to the trait and `GitCli`.

**Confidence: HIGH** — tested `git log --oneline --grep="^merge(" | sed 's/^merge(\([^)]*\)).*/\1/p'` and it correctly extracted session names from merge commits.

### Example 4: Skip Flow (reset squash merge)

When user chooses "skip", undo the failed squash merge:

```rust
// In merge_sessions() conflict handler:
ConflictAction::Skip => {
    // Undo the squash merge — restores worktree to pre-merge state
    git.reset_hard(worktree_path, "HEAD").await?;
    skipped_sessions.push(session.session_name.clone());
    continue; // next session in loop
}
```

`git reset --hard HEAD` after a conflicted squash merge cleanly removes both the staged non-conflicting changes and the conflicting files with markers. Verified experimentally.

### Example 5: Abort Keep/Rollback Prompt

```rust
let items = &[
    "Keep target branch (with successful merges so far)",
    "Roll back entirely (delete target branch)",
];
let choice = Select::with_theme(&ColorfulTheme::default())
    .with_prompt("Merge aborted. What would you like to do?")
    .items(items)
    .default(0)
    .interact_on(&Term::stderr())?;

match choice {
    0 => {
        // Keep: remove temp worktree but preserve target branch
        // Clean up the conflicted state first
        git.reset_hard(&temp_path, "HEAD").await?;
        git.worktree_remove(&temp_path, true).await?;
        git.worktree_prune().await?;
        // Target branch has N successfully merged sessions
    }
    _ => {
        // Rollback: remove temp worktree AND delete target branch
        git.reset_hard(&temp_path, "HEAD").await?;
        git.worktree_remove(&temp_path, true).await?;
        git.worktree_prune().await?;
        git.branch_delete(&target_branch, true).await?;
    }
}
```

---

## Git Operations Inventory

New `GitOps` methods needed for Phase 6:

| Method | Git Command | Purpose |
|--------|-------------|---------|
| `log_subjects(range)` | `git log --format="%s" <range>` | Resume detection — extract session names from merge commit subjects |
| `add_files(work_dir, paths)` | `git add <paths>` | Stage resolved conflict files (existing `add()` works — takes `&[&str]`) |

Methods already available that Phase 6 will use:

| Method | Purpose |
|--------|---------|
| `unmerged_files(work_dir)` | List conflicting files after failed squash merge |
| `reset_hard(work_dir, target)` | Undo failed squash merge (skip flow) |
| `add(work_dir, paths)` | Stage resolved files |
| `commit(work_dir, message)` | Commit after resolution |
| `branch_exists(name)` | Resume detection — check if target branch exists |
| `branch_delete(name, force)` | Rollback on abort |
| `worktree_remove(path, force)` | Cleanup temp worktree |
| `worktree_prune()` | Cleanup worktree metadata |
| `merge_squash(work_dir, source)` | The squash merge itself (no change needed — already returns `MergeConflict` with file list) |

**Only 1 new GitOps method** needed: `log_subjects`. Everything else is already implemented.

**Confidence: HIGH** — verified against `GitOps` trait in `crates/smelt-core/src/git/mod.rs`.

---

## Key Design Decisions (Recommendations for Claude's Discretion items)

### Conflict marker detection: Line scan with `starts_with`
Use `std::fs::read_to_string` + line iteration with `starts_with("<<<<<<<")` etc. No regex needed — conflict markers are fixed-prefix strings. Require all three markers in sequence to count as a hunk.

### Progress display: Printed lines (not inline updates)
Use `eprintln!`-style progress like the current merge loop does (`[1/5] Merging session 'foo'...`). Add conflict/skip/resolve status to the progress line. No cursor manipulation needed. Example: `[3/5] CONFLICT in 'bar' — resolved manually`.

### Already-merged session detection on resume
Parse `git log --format="%s" base..target` for commit subjects matching `merge(<name>):`. This uses the commit message template already established in Phase 5. Simple string parsing, no regex.

### Prompt styling
Use `dialoguer::Select` with `ColorfulTheme::default()` (consistent with existing dialoguer usage pattern). All prompts write to `Term::stderr()`.

### Skip triggers reset of failed squash merge
On skip: `git reset --hard HEAD` undoes the squash merge completely (both staged and unstaged changes). This is the correct behavior because a partial squash merge (non-conflicting changes staged, conflicting files unresolved) is not a valid state to continue from. The session's branch is preserved (not deleted) so the user can return to it later. Verified experimentally.

---

## Architectural Notes

### Where code lives

| Component | Crate | File |
|-----------|-------|------|
| `ConflictAction` enum | `smelt-core` | `crates/smelt-core/src/merge/types.rs` |
| `ConflictHandler` trait | `smelt-core` | `crates/smelt-core/src/merge/mod.rs` (or new `conflict.rs` submodule) |
| Conflict marker scanning | `smelt-core` | `crates/smelt-core/src/merge/conflict.rs` (new) |
| `merge_sessions()` changes | `smelt-core` | `crates/smelt-core/src/merge/mod.rs` |
| Interactive conflict handler | `smelt-cli` | `crates/smelt-cli/src/commands/merge.rs` |
| Resume detection logic | `smelt-core` | `crates/smelt-core/src/merge/mod.rs` |
| `log_subjects()` git op | `smelt-core` | `crates/smelt-core/src/git/{mod,cli}.rs` |

### MergeRunner signature change

`MergeRunner::run()` needs to accept a conflict handler. Options:

1. **Generic parameter:** `run<H: ConflictHandler>(&self, manifest, opts, handler: &H)` — matches `GitOps` pattern
2. **Trait object:** `run(&self, manifest, opts, handler: &dyn ConflictHandler)` — simpler but less testable

**Recommendation:** Generic parameter (option 1). Consistent with how `G: GitOps` is used on `MergeRunner<G>`. Could even be `MergeRunner<G, H>` but that's excessive — a method-level generic is sufficient since the handler is only used during `run()`.

### MergeOpts additions

`MergeOpts` needs a `verbose: bool` field for the `--verbose` flag that dumps full diff context on conflicts. No other opts changes needed — the `--target` and `--strategy` flags are already handled.

### MergeReport additions

`MergeReport` needs to distinguish between sessions that were skipped (never attempted) vs. skipped due to conflict. Current `sessions_skipped: Vec<String>` tracks pre-merge skips (failed/missing sessions). Add:

```rust
pub sessions_conflict_skipped: Vec<String>,  // skipped due to conflict
pub sessions_resolved: Vec<String>,          // resolved manually
```

Or better: change `MergeSessionResult` to include a `resolution: Option<ResolutionMethod>` field where `ResolutionMethod` is `Clean | Manual | Skipped`.

---

*Research complete. All domains investigated. Ready for planning.*
