# Phase 5: Merge Order Intelligence - Research

**Researched:** 2026-03-10
**Confidence baseline:** HIGH unless noted otherwise

---

## Standard Stack

### Table formatting: `comfy-table` (v7.x)

**Confidence: HIGH** (verified via Context7, official docs)

Use `comfy-table` for human-readable terminal table output. It is already a natural fit with the project's existing `console` crate dependency (both from the same ecosystem family).

- Zero derive macros needed -- build tables imperatively with `Table::new()`, `.set_header()`, `.add_row()`
- `ContentArrangement::Dynamic` auto-wraps content to terminal width
- Presets: `UTF8_FULL` with `UTF8_ROUND_CORNERS` for clean terminal output
- Renders via `Display` trait -- just `println!("{table}")`
- Minimal dependency footprint; `tty` feature auto-detects terminal width

**Why not `tabled`:** `tabled` uses a derive macro approach (`#[derive(Tabled)]`) that works well for fixed-schema data but is heavier for dynamic tables where column layout varies per output mode (file list, overlap matrix, ordered list). `comfy-table` is more natural for imperative table building.

**Crate version:** `comfy-table = "7"` (add to workspace dependencies)

### JSON output: `serde_json`

**Confidence: HIGH** (verified via Context7, serde already in workspace)

Use `serde_json` for `--json` structured output. `serde` is already a workspace dependency; `serde_json` is the standard companion.

- `#[derive(Serialize)]` on output structs
- `serde_json::to_writer_pretty(io::stdout(), &plan)` for pretty-printed JSON to stdout
- No need for `to_string` intermediate -- write directly to stdout

**Crate version:** `serde_json = "1"` (add to workspace dependencies)

### No new crates needed for algorithms

The overlap scoring algorithm is straightforward set intersection. Rust's `std::collections::HashSet` provides:
- `HashSet::intersection()` for pairwise overlap
- `HashSet::len()` for set sizes

No graph algorithm crate is needed. The ordering problem is not a graph problem -- it is a greedy selection: sort session pairs by overlap score (ascending), then merge in that order. This is O(n^2) pairwise comparison where n = number of sessions, which is trivially small (typically 2-10 sessions).

---

## Architecture Patterns

### Strategy pattern: enum dispatch (not trait objects)

**Confidence: HIGH** (codebase convention analysis)

Use an enum for merge ordering strategies, not trait objects. Rationale:

1. **Codebase precedent:** The project uses enums extensively (`SessionStatus`, `FailureMode`, `ScriptStep`, `SmeltError`). No trait objects are used anywhere in the codebase.
2. **Only two variants:** With exactly two strategies (and extensibility via `#[non_exhaustive]`), enum dispatch is simpler than `Box<dyn Strategy>`.
3. **Serde compatibility:** Enums derive `Serialize`/`Deserialize` trivially for manifest TOML config. Trait objects require custom serde impls.
4. **Edition 2024:** No concerns with enum dispatch on edition 2024.

```rust
/// Strategy for determining merge order.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum MergeOrderStrategy {
    /// Order sessions by manifest order (completion time proxy).
    #[default]
    CompletionTime,
    /// Order sessions to minimize file overlap between consecutive merges.
    FileOverlap,
}
```

### Integration point: MergeOpts + Manifest

The strategy flows through two paths:
1. **Manifest TOML:** New optional field `merge_strategy` on `ManifestMeta`
2. **CLI flag:** `--strategy <completion-time|file-overlap>` on `smelt merge`
3. **MergeOpts:** New field `strategy: Option<MergeOrderStrategy>` -- CLI value overrides manifest value; if neither set, default is `CompletionTime`

`MergeOpts` is already `#[non_exhaustive]`, so adding the field is non-breaking.

### File set gathering: new `GitOps::diff_name_only` method

**Confidence: HIGH** (verified against existing `GitOps` trait)

The existing `diff_numstat` returns `Vec<(usize, usize, String)>` -- it could be reused by discarding the numeric columns, but a dedicated `diff_name_only` method is cleaner:

```rust
/// List file paths changed between two refs.
fn diff_name_only(
    &self,
    from_ref: &str,
    to_ref: &str,
) -> impl Future<Output = Result<Vec<String>>> + Send;
```

Implementation: `git diff --name-only <from_ref> <to_ref>`, split stdout by newlines.

This is preferred over reusing `diff_numstat` because:
- Avoids parsing numeric columns we don't need
- Binary files show `-` in numstat but work fine in `--name-only`
- Cleaner intent in calling code

### Ordering algorithm: greedy overlap minimization

For the file-overlap strategy:

1. **Gather file sets:** For each session, compute `merge_base(base_ref, session_branch)` then `diff_name_only(merge_base, session_branch)` to get `HashSet<String>` of changed files.

2. **Compute pairwise overlap:** For each pair (i, j), overlap = `|files_i intersect files_j|`. Store in a flat Vec or HashMap since n is small.

3. **Greedy ordering:** Start with the session that has the least total overlap with all others. At each step, pick the session with the least overlap against the *already-merged set* of files. This is a greedy heuristic, not optimal, but optimal ordering is NP-hard and unnecessary for n < 20.

4. **Fallback:** If all pairwise overlaps are identical (e.g., all sessions touch the same files), log a message to stderr and fall back to completion-time ordering.

5. **Tiebreaker:** When overlap scores are equal, preserve manifest order (stable sort). This ensures determinism.

### Plan subcommand vs dry-run flag

**Recommendation: `merge plan` subcommand** (Claude's discretion area)

Rationale:
- A plan command has different output semantics than a merge -- it never mutates state
- It naturally accommodates `--json` without conflating merge output with plan output
- Consistent with CLI conventions: `terraform plan` / `terraform apply`
- Avoids the `--dry-run` flag ambiguity (does it create the branch but not push? does it do a trial merge?)

```
smelt merge plan <manifest> [--strategy <s>] [--json]
smelt merge <manifest> [--target <branch>] [--strategy <s>]
```

---

## Don't Hand-Roll

| Problem | Use Instead | Why |
|---|---|---|
| Terminal table formatting | `comfy-table` | Dynamic column sizing, terminal width detection, presets |
| JSON serialization | `serde_json` + `#[derive(Serialize)]` | Standard, already in ecosystem |
| Set intersection | `std::collections::HashSet` | Built-in, zero allocation for intersection iterator |
| Graph/scheduling algorithms | Greedy loop over Vec | n < 20 sessions; no graph crate needed |
| CLI argument parsing for new subcommand | `clap` derive macros | Already used throughout `smelt-cli` |

---

## Common Pitfalls

### 1. Merge base calculation per session

**Risk: HIGH**

Each session may have a different base ref (via `SessionDef::base_ref` override). The overlap computation must use each session's actual merge base, not a single global base. The correct sequence is:

```
for each session:
  effective_base = session.base_ref.unwrap_or(manifest.base_ref)
  merge_base = git.merge_base(effective_base, session_branch)
  files = git.diff_name_only(merge_base, session_branch)
```

Getting this wrong means the file sets will be incorrect, leading to wrong overlap scores.

### 2. Session branch may not exist yet

The overlap strategy needs access to session branches. If a session hasn't been run yet (no worktree/branch created), the file set is empty. The plan command should handle this gracefully:
- Sessions with no branch: show as "no data" in the plan output, excluded from overlap scoring
- Only completed sessions contribute to ordering

### 3. `diff_name_only` on renamed files

`git diff --name-only` includes both old and new names for renamed files by default. With `--no-renames`, it shows the delete + add separately. **Use default behavior** (with renames) because:
- Two sessions renaming the same file IS a conflict
- The new name appearing in both sets correctly indicates overlap

### 4. Empty file sets

A session with no file changes (e.g., only commit message changes, or empty commits) produces an empty file set. This has zero overlap with everything and should naturally sort first in the overlap strategy.

### 5. JSON output schema stability

The `--json` output becomes a contract for CI/agent consumers. Derive `Serialize` on dedicated output types (not reuse internal types) so the schema can evolve independently:

```rust
#[derive(Serialize)]
struct MergePlan {
    strategy: String,
    sessions: Vec<PlannedSession>,
    pairwise_overlaps: Vec<OverlapEntry>,  // only present for file-overlap strategy
}
```

### 6. Async overhead for plan command

The plan command makes N+1 git calls (1 merge-base + 1 diff per session). These are sequential because they shell out to git. For N < 20, this completes in under a second. No need for parallelization, but do reuse the existing `GitOps` async pattern for consistency.

---

## Code Examples

### Adding `diff_name_only` to `GitOps` trait and `GitCli`

```rust
// In git/mod.rs - add to GitOps trait:
/// List file paths changed between two refs.
fn diff_name_only(
    &self,
    from_ref: &str,
    to_ref: &str,
) -> impl Future<Output = Result<Vec<String>>> + Send;

// In git/cli.rs - add to GitCli impl:
async fn diff_name_only(&self, from_ref: &str, to_ref: &str) -> Result<Vec<String>> {
    let output = self.run(&["diff", "--name-only", from_ref, to_ref]).await?;
    if output.is_empty() {
        return Ok(Vec::new());
    }
    Ok(output.lines().map(|l| l.to_string()).collect())
}
```

### Overlap scoring with HashSet

```rust
use std::collections::HashSet;

struct SessionFiles {
    session_name: String,
    files: HashSet<String>,
}

fn pairwise_overlap(a: &SessionFiles, b: &SessionFiles) -> usize {
    a.files.intersection(&b.files).count()
}

fn total_overlap(session: &SessionFiles, others: &[SessionFiles]) -> usize {
    others.iter().map(|o| pairwise_overlap(session, o)).sum()
}
```

### Greedy ordering algorithm

```rust
fn order_by_overlap(mut sessions: Vec<SessionFiles>) -> Vec<SessionFiles> {
    let mut ordered = Vec::with_capacity(sessions.len());
    let mut merged_files: HashSet<String> = HashSet::new();

    while !sessions.is_empty() {
        // Pick session with least overlap against already-merged files
        let best_idx = sessions
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| s.files.intersection(&merged_files).count())
            .map(|(i, _)| i)
            .unwrap(); // safe: sessions is non-empty

        let chosen = sessions.swap_remove(best_idx);
        merged_files.extend(chosen.files.iter().cloned());
        ordered.push(chosen);
    }

    ordered
}
```

Note: `swap_remove` changes order of remaining elements but that's fine since we're selecting by min overlap each iteration, not relying on position.

### comfy-table plan output

```rust
use comfy_table::{Table, ContentArrangement, presets::UTF8_FULL, modifiers::UTF8_ROUND_CORNERS};

fn print_merge_plan(plan: &MergePlan) {
    // Session file sets table
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["#", "Session", "Files Changed", "Strategy Score"]);

    for (i, session) in plan.sessions.iter().enumerate() {
        table.add_row(vec![
            format!("{}", i + 1),
            session.name.clone(),
            format!("{}", session.file_count),
            format!("{}", session.overlap_score),
        ]);
    }

    println!("{table}");
}
```

### MergeOrderStrategy enum with manifest TOML integration

```rust
// In manifest.rs - add to ManifestMeta:
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestMeta {
    pub name: String,
    #[serde(default = "default_base_ref")]
    pub base_ref: String,
    /// Merge ordering strategy. Default: completion-time.
    #[serde(default)]
    pub merge_strategy: Option<MergeOrderStrategy>,
}
```

TOML usage:
```toml
[manifest]
name = "my-project"
base_ref = "main"
merge_strategy = "file-overlap"
```

### CLI subcommand structure

```rust
// In main.rs Commands enum:
Merge {
    #[command(subcommand)]
    command: MergeCommands,
},

#[derive(Subcommand)]
enum MergeCommands {
    /// Preview merge order without executing
    Plan {
        /// Path to the session manifest file
        manifest: String,
        /// Merge ordering strategy
        #[arg(long, value_enum)]
        strategy: Option<MergeOrderStrategy>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Execute the merge
    Run {
        /// Path to the session manifest file
        manifest: String,
        /// Override target branch name
        #[arg(long)]
        target: Option<String>,
        /// Merge ordering strategy
        #[arg(long, value_enum)]
        strategy: Option<MergeOrderStrategy>,
    },
}
```

**Breaking change note:** Changing `Merge` from a leaf command to a subcommand parent is a breaking CLI change. Since we're at v0.1.0 (pre-stability), this is acceptable. The existing `smelt merge <manifest>` becomes `smelt merge run <manifest>`.

---

## New Dependencies Summary

| Crate | Version | Where | Purpose |
|---|---|---|---|
| `comfy-table` | `"7"` | `smelt-cli` | Terminal table formatting for plan output |
| `serde_json` | `"1"` | `smelt-cli` | JSON serialization for `--json` flag |

Both are workspace dependencies. `serde_json` may also be useful in `smelt-core` for future JSON output of `MergeReport`, but for Phase 5 it's only needed in the CLI crate.

No changes to `smelt-core` dependencies beyond the code additions (new `GitOps` method, strategy enum, ordering logic).

---

## Open Questions for Planner

1. **Subcommand migration path:** Should `smelt merge run` be the new name, or keep `smelt merge` as an alias for `smelt merge run` for backward compatibility? The `clap` `visible_alias` feature could help here.

2. **Plan output scope:** Should `merge plan` also validate session states (completed/running/failed) or only show ordering? Validating states adds value (user sees which sessions would be skipped) but couples plan to worktree state.

---

*Phase: 05-merge-order*
*Researched: 2026-03-10*
