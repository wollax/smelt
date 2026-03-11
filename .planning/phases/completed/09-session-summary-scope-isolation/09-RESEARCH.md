# Phase 9: Session Summary & Scope Isolation - Research

**Researched:** 2026-03-11
**Confidence:** HIGH (all findings verified against codebase)

## Standard Stack

All libraries are already workspace dependencies. No new crates required.

| Purpose | Library | Version | Notes |
|---------|---------|---------|-------|
| Glob matching | `globset` | 0.4 | Already in smelt-core. Use `GlobSet` (compiled set) for matching multiple patterns efficiently |
| Table output | `comfy-table` | 7 | Already in smelt-cli. UTF8_FULL + UTF8_ROUND_CORNERS preset established |
| JSON output | `serde_json` | 1 | Already in both crates |
| Serialization | `serde` | 1 (derive) | Already everywhere |
| Git operations | `GitOps` trait | internal | `diff_numstat` and `log_subjects` already implemented |
| State persistence | `RunStateManager` | internal | `.smelt/runs/<run_id>/` directory pattern established |
| TOML manifest | `toml` + `serde` | internal | `Manifest`/`ManifestMeta` already parsed with serde Deserialize |

**Confidence: HIGH** - Every library is already a workspace dependency with established usage patterns.

## Architecture Patterns

### 1. Summary analysis module placement

**Recommendation:** New `summary` module in `smelt-core/src/summary/mod.rs` (sibling to `merge/`, `orchestrate/`).

Rationale:
- Summary analysis is a distinct domain concern (post-merge reporting), not merge logic or orchestration logic.
- The `merge/` module already has `types.rs`, `ordering.rs`, `conflict.rs`, `ai_handler.rs` — adding summary there conflates concerns.
- A dedicated module keeps the `SummaryReport` type, scope-checking logic, and analysis functions self-contained.
- Module structure: `summary/mod.rs` (public API + analysis), `summary/types.rs` (SummaryReport, SessionSummary, ScopeViolation structs).

**Confidence: HIGH**

### 2. Data flow: collect_sessions -> diff_numstat -> summary analysis

Current `collect_sessions()` in `merge/mod.rs` (line 286) calls `diff_name_only` per session and returns `CompletedSession` with `changed_files: HashSet<String>`. The CONTEXT.md decision says to extend this to include `diff_numstat` data.

**Recommended approach:**
- Add a new standalone function (not inside `MergeRunner`) that collects per-session `diff_numstat` data. This keeps `collect_sessions` focused on merge ordering (it only needs file names for overlap scoring).
- The summary analysis function takes the manifest + per-session numstat data and produces a `SummaryReport`.
- The orchestrator calls this analysis function pre-merge, stores the result, and displays it post-merge.

Alternative considered and rejected: modifying `CompletedSession` to carry numstat data. This would couple merge ordering to summary concerns and bloat the struct with data only summary needs.

**Confidence: HIGH**

### 3. Scope violation detection with GlobSet

The `globset` crate provides `GlobSet` — a compiled set of multiple glob patterns that can be matched against a path in one call. This is the correct abstraction for scope checking.

Pattern for building a scope checker:

```rust
use globset::{Glob, GlobSet, GlobSetBuilder};

fn build_scope_matcher(
    file_scope: &[String],
    shared_files: &[String],
) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in file_scope.iter().chain(shared_files.iter()) {
        builder.add(Glob::new(pattern)?);
    }
    builder.build()
}

// Usage: matcher.is_match(file_path) -> bool
```

Key behaviors (verified from codebase usage at `manifest.rs:173`):
- `Glob::new(pattern)` validates the pattern (already done during manifest validation).
- `GlobSet::is_match(&self, path)` returns bool. Case-sensitive by default (matches CONTEXT.md decision).
- `GlobSetBuilder` allows combining session `file_scope` globs with manifest-level `shared_files` globs into a single matcher.

**Confidence: HIGH**

### 4. Manifest extension: `shared_files` field

Add `shared_files: Option<Vec<String>>` to `ManifestMeta` (in `session/manifest.rs:20`). Follows the same pattern as `merge_strategy`, `on_failure`, `parallel_by_default` — optional manifest-level field with serde default.

Validation: validate glob patterns the same way `file_scope` is validated (line 171-179 of manifest.rs). Reuse the same `globset::Glob::new()` check.

**Confidence: HIGH**

### 5. SummaryReport struct design (Claude's Discretion area)

**Recommendation:**

```rust
#[derive(Debug, Clone, Serialize)]
pub struct SummaryReport {
    pub sessions: Vec<SessionSummary>,
    pub violations: Vec<ScopeViolation>,
    pub totals: SummaryTotals,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub session_name: String,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub file_details: Vec<FileStat>,       // populated for --verbose / --json
    pub commit_messages: Vec<String>,       // populated for --verbose / --json
}

#[derive(Debug, Clone, Serialize)]
pub struct FileStat {
    pub path: String,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopeViolation {
    pub session_name: String,
    pub file: String,
    pub insertions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SummaryTotals {
    pub total_files_changed: usize,
    pub total_insertions: usize,
    pub total_deletions: usize,
    pub total_violations: usize,
}
```

Design notes:
- `file_details` and `commit_messages` are always populated in the struct but only displayed in verbose/JSON mode. This avoids needing two code paths for data collection.
- `ScopeViolation` is per-file, not per-session, giving maximum granularity in JSON output. The CLI groups them by session for display.
- All types derive `Serialize` for `--json` output. No need for `Deserialize` — these are output-only types (never loaded from disk as typed structs; the persisted `summary.json` is just the serialized form).

**Confidence: HIGH**

### 6. Standalone `smelt summary` command

Add a new top-level CLI command following the established pattern:
- New file: `smelt-cli/src/commands/summary.rs`
- New enum variant in `Commands` (main.rs:25)
- Arguments: `<manifest>` (required), `--run-id` (optional), `--verbose`, `--json`
- Default behavior: load latest completed run's `summary.json` from `.smelt/runs/`

For finding the latest completed run, extend `RunStateManager` with a `find_latest_completed_run(manifest_name)` method (inverse of the existing `find_incomplete_run`).

**Confidence: HIGH**

### 7. comfy-table formatting (Claude's Discretion area)

**Recommendation for compact table:**

```
╭──────────────┬───────┬────────┬────────╮
│ Session      ┆ Files ┆ +Lines ┆ -Lines │
╞══════════════╪═══════╪════════╪════════╡
│ add-auth     ┆     3 ┆    142 ┆     12 │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┤
│ fix-tests    ┆     1 ┆      8 ┆      3 │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┤
│ Total        ┆     4 ┆    150 ┆     15 │
╰──────────────┴───────┴────────┴────────╯
```

- Right-align numeric columns (Files, +Lines, -Lines) via `column.set_cell_alignment(CellAlignment::Right)` on columns 1, 2, 3.
- Include a "Total" row as the final row (not a table footer — comfy-table doesn't have native footer support).
- Matches the existing UTF8_FULL + UTF8_ROUND_CORNERS style used in `orchestrate.rs:636` and `merge.rs`.

**Verbose mode:** Per-session blocks with file lists. No truncation recommended — the user explicitly opted into verbose. Format:

```
Session: add-auth (3 files, +142/-12)
  Commits:
    - Add authentication handler
    - Add auth middleware
  Files:
    src/auth/handler.rs    +98  -0
    src/auth/middleware.rs  +36  -4
    src/lib.rs              +8  -8
```

**Confidence: HIGH**

### 8. Commit message collection

`GitOps::log_subjects(range)` already implemented (`git/cli.rs:312`). Use range `{base_ref}..{branch_name}` to get all commit subjects for a session. This is the same range pattern used by `diff_numstat` and `diff_name_only`.

**Confidence: HIGH**

### 9. Summary persistence

Store as `summary.json` alongside `state.json` in `.smelt/runs/<run_id>/`. Use `RunStateManager` to resolve the directory path:

```rust
// In RunStateManager, add:
pub fn summary_path(&self, run_id: &str) -> PathBuf {
    self.runs_dir.join(run_id).join("summary.json")
}
```

The `SummaryReport` is serialized with `serde_json::to_string_pretty` and written to this path. For the standalone `smelt summary` command, it's loaded back with `serde_json::from_str` (so `SummaryReport` also needs `Deserialize` — revising the recommendation in section 5: add `Deserialize` to all summary types).

**Confidence: HIGH** (corrected mid-research)

### 10. Integration into orchestrate flow

The orchestrate executor (`executor.rs`) calls `merge_phase()` at line 174. Summary analysis should happen:
1. **Before** `merge_phase()`: collect per-session `diff_numstat` data (clean per-session attribution against base ref).
2. **After** `merge_phase()` returns: persist `summary.json` and display the summary table.

This matches the CONTEXT.md decision: "pre-merge analysis, shown retrospectively after merge completes."

The `Orchestrator::run()` and `Orchestrator::resume()` methods both converge on `merge_phase()`, so the summary collection should be a separate method called from both paths, just before `merge_phase()`.

**Confidence: HIGH**

## Don't Hand-Roll

| Problem | Use Instead | Why |
|---------|-------------|-----|
| Glob pattern matching | `globset::GlobSet` | Handles `**`, `?`, character classes, edge cases. Already a dependency. |
| Table formatting | `comfy-table` with UTF8_FULL preset | Already established pattern in codebase. Handles column widths, alignment, Unicode. |
| JSON serialization | `serde_json` with `#[derive(Serialize)]` | Already used everywhere. Don't format JSON manually. |
| Git diff statistics | `GitOps::diff_numstat` | Already implemented and tested. Parses `git diff --numstat` output. |
| Commit message listing | `GitOps::log_subjects` | Already implemented. Parses `git log --format=%s`. |
| Run directory management | `RunStateManager` | Already handles `.smelt/runs/` lifecycle. Extend, don't duplicate. |
| Glob validation | `globset::Glob::new()` | Already used in manifest validation (manifest.rs:173). Reuse same pattern for `shared_files`. |

## Common Pitfalls

### 1. Binary files in diff_numstat
`git diff --numstat` outputs `-\t-\tfilename` for binary files. The existing `GitCli::diff_numstat` implementation (cli.rs:336) already handles this by using `.parse::<usize>().ok()` which returns `None` for `-`, causing `filter_map` to skip binaries. However, **scope violation checking must still account for binary files** — a binary file outside scope is still a violation even though it has no line counts.

**Mitigation:** Use `diff_name_only` alongside `diff_numstat`, or add a separate binary-aware numstat variant. Simplest: run `diff_name_only` for the full file list (scope checking), and `diff_numstat` for line statistics. The union gives complete coverage.

**Confidence: HIGH** — verified in `git/cli.rs:336-347`.

### 2. Session with no file_scope means "everything in scope"
Per CONTEXT.md decision: `file_scope = None` means no scope checking (everything is in-scope). The implementation must short-circuit scope checking when `file_scope` is `None`, not treat it as "nothing matches."

**Confidence: HIGH** — explicit decision in CONTEXT.md.

### 3. Empty shared_files default
`shared_files` defaults to empty (`Option<Vec<String>>` with `None` or empty vec). When building the `GlobSet`, if both `file_scope` and `shared_files` are empty, the `GlobSet` matches nothing — but this case is unreachable because `file_scope = None` short-circuits before `GlobSet` is built.

**Confidence: HIGH**

### 4. Path normalization
`diff_numstat` and `diff_name_only` return paths relative to the repo root (e.g., `src/auth/handler.rs`). Glob patterns in `file_scope` are also relative (e.g., `src/auth/**`). No normalization needed — both are repo-relative. However, be aware that `diff_numstat` can return paths with `{old_path => new_path}` format for renames. The existing implementation splits on `\t` which handles this correctly (the rename format appears in the third column).

**Confidence: MEDIUM** — rename path format needs verification during implementation. The `git diff --numstat` rename output is `insertions\tdeletions\told_path => new_path` or `insertions\tdeletions\t{prefix/old => new}/suffix` depending on git config. The current parser takes the full third column as the filename, which would include the rename syntax. May need to extract just the new path.

### 5. Scope checking uses file_scope from manifest, not worktree state
`SessionDef.file_scope` (manifest) and `WorktreeState.file_scope` (worktree state file) both exist and should be identical. Use the manifest as the source of truth since the summary command takes a manifest path. The worktree state is secondary.

**Confidence: HIGH**

### 6. diff_numstat against base_ref must use the correct per-session base
Each session can override `base_ref`. The `collect_sessions` function already handles this (line 325-328 of merge/mod.rs). Summary collection must follow the same pattern: `session_def.base_ref.unwrap_or(manifest.manifest.base_ref)`.

**Confidence: HIGH** — verified in codebase.

### 7. Summary must survive merge failure
CONTEXT.md: "data survives merge failure." The summary is computed pre-merge and persisted before merge begins. If merge fails, the summary.json is already on disk. The standalone `smelt summary` command can read it regardless of run outcome.

**Confidence: HIGH**

### 8. Thread safety for SummaryReport
Summary analysis is purely computational (no async, no I/O after data collection). The `diff_numstat` calls are async but can be collected sequentially or concurrently before the synchronous analysis. No mutex or Arc needed on `SummaryReport` itself.

**Confidence: HIGH**

## Code Examples

### Building a GlobSet for scope checking

```rust
use globset::{Glob, GlobSet, GlobSetBuilder};

/// Build a combined scope matcher from session file_scope and manifest shared_files.
/// Returns None if file_scope is None (everything in scope).
fn build_scope_matcher(
    file_scope: Option<&[String]>,
    shared_files: &[String],
) -> Result<Option<GlobSet>> {
    let Some(scopes) = file_scope else {
        return Ok(None); // No scope defined = everything in scope
    };

    let mut builder = GlobSetBuilder::new();
    for pattern in scopes.iter().chain(shared_files.iter()) {
        builder.add(Glob::new(pattern).map_err(|e| {
            SmeltError::ManifestParse(format!("invalid glob '{pattern}': {e}"))
        })?);
    }
    Ok(Some(builder.build().map_err(|e| {
        SmeltError::ManifestParse(format!("failed to build glob set: {e}"))
    })?))
}

/// Check files against scope. Returns list of out-of-scope files.
fn find_violations(
    files: &[String],
    matcher: Option<&GlobSet>,
) -> Vec<String> {
    let Some(matcher) = matcher else {
        return Vec::new(); // No scope = no violations
    };
    files.iter()
        .filter(|f| !matcher.is_match(f.as_str()))
        .cloned()
        .collect()
}
```

### Collecting per-session diff_numstat data

```rust
/// Per-session numstat data collected before merge.
struct SessionNumstat {
    session_name: String,
    base_ref: String,
    branch_name: String,
    file_scope: Option<Vec<String>>,
    stats: Vec<(usize, usize, String)>,  // (insertions, deletions, path)
    all_files: Vec<String>,               // from diff_name_only (includes binaries)
    commit_messages: Vec<String>,
}

async fn collect_session_numstats<G: GitOps>(
    git: &G,
    manifest: &Manifest,
    smelt_dir: &Path,
) -> Result<Vec<SessionNumstat>> {
    // Similar to collect_sessions but focused on summary data
    // Uses WorktreeState to get branch_name, then:
    //   git.diff_numstat(base_ref, branch_name)
    //   git.diff_name_only(base_ref, branch_name)
    //   git.log_subjects(&format!("{base_ref}..{branch_name}"))
}
```

### comfy-table summary with right-aligned numerics

```rust
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Table, CellAlignment};

fn format_summary_table(report: &SummaryReport) -> String {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec!["Session", "Files", "+Lines", "-Lines"]);

    // Right-align numeric columns
    for col_idx in 1..=3 {
        if let Some(col) = table.column_mut(col_idx) {
            col.set_cell_alignment(CellAlignment::Right);
        }
    }

    for session in &report.sessions {
        table.add_row(vec![
            session.session_name.clone(),
            session.files_changed.to_string(),
            format!("+{}", session.insertions),
            format!("-{}", session.deletions),
        ]);
    }

    // Totals row
    table.add_row(vec![
        "Total".to_string(),
        report.totals.total_files_changed.to_string(),
        format!("+{}", report.totals.total_insertions),
        format!("-{}", report.totals.total_deletions),
    ]);

    table.to_string()
}
```

### Persisting and loading summary.json

```rust
// In RunStateManager:
pub fn save_summary(&self, run_id: &str, report: &SummaryReport) -> Result<()> {
    let path = self.runs_dir.join(run_id).join("summary.json");
    let json = serde_json::to_string_pretty(report)
        .map_err(|e| SmeltError::Orchestration {
            message: format!("failed to serialize summary: {e}"),
        })?;
    std::fs::write(&path, json)
        .map_err(|e| SmeltError::io("writing summary", &path, e))
}

pub fn load_summary(&self, run_id: &str) -> Result<SummaryReport> {
    let path = self.runs_dir.join(run_id).join("summary.json");
    let json = std::fs::read_to_string(&path)
        .map_err(|e| SmeltError::io("reading summary", &path, e))?;
    serde_json::from_str(&json)
        .map_err(|e| SmeltError::Orchestration {
            message: format!("failed to deserialize summary: {e}"),
        })
}
```

## Key Codebase Integration Points

| What | Where | Action |
|------|-------|--------|
| `ManifestMeta` | `session/manifest.rs:20` | Add `shared_files: Option<Vec<String>>` field |
| `Manifest::validate()` | `session/manifest.rs:131` | Validate `shared_files` globs same as `file_scope` |
| `collect_sessions()` | `merge/mod.rs:286` | No changes needed — summary uses its own data collection |
| `RunStateManager` | `orchestrate/state.rs:26` | Add `save_summary`, `load_summary`, `find_latest_completed_run` |
| `Orchestrator::run()` | `orchestrate/executor.rs:46` | Insert summary collection before `merge_phase()` |
| `Orchestrator::resume()` | `orchestrate/executor.rs:186` | Same insertion point for resume path |
| `format_orchestration_summary()` | `cli/commands/orchestrate.rs:625` | Append session summary table after merge summary |
| `Commands` enum | `cli/src/main.rs:24` | Add `Summary` variant |
| `commands/mod.rs` | `cli/src/commands/mod.rs:1` | Add `pub mod summary;` |
| `lib.rs` re-exports | `smelt-core/src/lib.rs:1` | Add `pub mod summary;` and re-export key types |

---

*Phase: 09-session-summary-scope-isolation*
*Research completed: 2026-03-11*
