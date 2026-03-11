# Phase 9: Session Summary & Scope Isolation - Context

**Gathered:** 2026-03-11
**Status:** Ready for planning

<domain>
## Phase Boundary

After sessions complete and merge, provide a structured summary of what each agent contributed (files changed, lines added/removed, commit messages). Verify that agents stayed within their assigned scope — flag sessions that modified files outside their task description. Scope violations are advisory, not enforcement.

Requirements: ORCH-02, ORCH-03

</domain>

<decisions>
## Implementation Decisions

### Scope violation detection logic
- No `file_scope` defined on a session = everything in scope (no violations possible). Scope checking is opt-in per session.
- Manifest-level `shared_files` glob list (optional, defaults to empty). Files matching `shared_files` are always considered in-scope for all sessions (e.g., `lib.rs`, `Cargo.toml`).
- All paths from `diff_numstat` go through the same glob check — deletions, renames, additions, and modifications are treated identically.
- Glob matching is case-sensitive (platform default).
- A file is in-scope if it matches any `file_scope` glob OR any `shared_files` glob. Otherwise it's a violation.

### Summary output design
- Hybrid layout: compact comfy-table summary (Session | Files | +Lines | -Lines columns), followed by a separate violations section listing only sessions with out-of-scope files.
- Violations section omitted entirely when zero violations exist (no noise).
- `--verbose` switches from table to per-session blocks showing all files with line counts and inline violation callouts.
- `--json` outputs a single `SummaryReport` struct containing both stats and violations.
- Summary always shown after `orchestrate run` completes (no flag needed). Also available as standalone `smelt summary <manifest>` command.

### Violation severity & messaging
- All violations treated equally — no minor/major distinction. A violation is a violation.
- Neutral/factual tone: "2 files outside scope" — no alarmist language.
- Violations do NOT affect exit code. Exit 0 on success even with violations. CI can parse JSON output for programmatic detection.
- Violations do NOT appear in merge commit messages. Summary output and JSON are the record.

### Summary timing & pipeline position
- Pre-merge analysis: per-session `diff_numstat` against base ref, computed before merge begins. Gives clean per-session attribution and data survives merge failure.
- Summary shown retrospectively after merge completes (does not gate or block merge).
- Extend existing `collect_sessions()` to include `diff_numstat` data — one git traversal, shared between merge ordering and summary.
- Summary data persisted as separate `summary.json` in `.smelt/runs/<run_id>/` alongside `state.json`.
- Standalone `smelt summary` defaults to latest completed run. Optional `--run-id` for specific runs.

### Claude's Discretion
- `SummaryReport` struct design and field naming
- Exact comfy-table column formatting and alignment
- How `--verbose` file list truncation works (if at all)
- Internal organization of summary analysis code (separate module or integrated into existing)
- How commit messages are collected and displayed in verbose mode

</decisions>

<specifics>
## Specific Ideas

- Hybrid table follows existing comfy-table style (UTF8_FULL + UTF8_ROUND_CORNERS) used by merge plan output
- `shared_files` in `[manifest]` section of TOML manifest alongside existing fields like `merge_strategy`
- Reuse `diff_numstat` (already implemented in GitOps) as the single data source powering both file counts and scope checking
- `collect_sessions()` already calls `diff_name_only` per session — extend to `diff_numstat` for richer data

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 09-session-summary-scope-isolation*
*Context gathered: 2026-03-11*
