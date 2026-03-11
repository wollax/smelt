---
phase: 09-session-summary-scope-isolation
plan: 01
subsystem: summary
tags: [summary, scope, globset, manifest, types]
dependency-graph:
  requires: [08-orchestration-plan-task-graph]
  provides: [summary-types, scope-checking, shared-files-manifest]
  affects: [09-02, 09-03]
tech-stack:
  added: []
  patterns: [GlobSet-based-scope-matching, opt-in-scope-checking]
key-files:
  created:
    - crates/smelt-core/src/summary/mod.rs
    - crates/smelt-core/src/summary/types.rs
    - crates/smelt-core/src/summary/scope.rs
  modified:
    - crates/smelt-core/src/session/manifest.rs
    - crates/smelt-core/src/lib.rs
    - crates/smelt-core/src/session/runner.rs
    - crates/smelt-core/src/merge/mod.rs
    - crates/smelt-core/src/orchestrate/executor.rs
    - crates/smelt-core/src/orchestrate/dag.rs
decisions:
  - id: shared-files-serde-default
    summary: "shared_files uses #[serde(default)] for backward-compatible empty Vec"
  - id: scope-check-error-fallback
    summary: "If GlobSet build fails at runtime (should not happen after validation), treat all files as violations"
metrics:
  duration: ~4 minutes
  completed: 2026-03-11
---

# Phase 09 Plan 01: Summary Foundation — Types, Manifest Extension, Scope Checking

**One-liner:** ManifestMeta shared_files with glob validation, summary report types (SummaryReport/SessionSummary/ScopeViolation/FileStat/SummaryTotals), GlobSet-based scope checking with opt-in semantics.

## What Was Done

### Task 1: Manifest shared_files extension and summary types
- Added `shared_files: Vec<String>` field to `ManifestMeta` with `#[serde(default)]` for backward compatibility
- Added glob validation for `shared_files` patterns in `Manifest::validate()`, placed after the session loop and before depends_on validation
- Created `crates/smelt-core/src/summary/` module with three files:
  - `mod.rs` — module declarations and public re-exports
  - `types.rs` — `FileStat`, `ScopeViolation`, `SessionSummary`, `SummaryTotals`, `SummaryReport` with Serialize/Deserialize
  - `scope.rs` — `check_scope()` function with GlobSet matching
- Added `pub mod summary` and re-exports to `lib.rs`
- Updated 5 files with direct `ManifestMeta` construction to include `shared_files: vec![]`
- Added 3 new manifest tests: `parse_manifest_with_shared_files`, `shared_files_defaults_to_empty`, `validate_rejects_invalid_shared_files_glob`

### Task 2: Scope checking logic with GlobSet
- Implemented `check_scope()` — returns empty Vec when `file_scope` is None (opt-in short-circuit)
- Implemented `build_scope_matcher()` — builds GlobSet from file_scope + shared_files patterns
- Files matching shared_files globs are always in-scope for all sessions
- ScopeViolation captures session name, file path, and the session's file_scope patterns (not shared_files)
- 7 unit tests covering all edge cases: no scope, all in scope, violations, shared override, multiple patterns, empty scope, violation field capture

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated all direct ManifestMeta constructions across codebase**
- **Found during:** Task 1
- **Issue:** Adding `shared_files` field to ManifestMeta broke 4 test helper functions in runner.rs, merge/mod.rs, orchestrate/executor.rs, and orchestrate/dag.rs that directly construct ManifestMeta
- **Fix:** Added `shared_files: vec![]` to all direct constructions
- **Files modified:** runner.rs, merge/mod.rs, executor.rs, dag.rs
- **Commit:** ce09522

## Decisions Made

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | `shared_files` uses `#[serde(default)]` | Backward-compatible — existing manifests parse without the field |
| 2 | Scope check error fallback treats all files as violations | Defensive — globs validated at parse time, runtime failure is unexpected |

## Test Results

- **208 total tests pass** (all existing + 10 new)
- **New manifest tests:** 3 (shared_files parsing, default, invalid glob)
- **New scope tests:** 7 (all edge cases covered)
- **Clippy:** Clean with `-D warnings`

## Commits

| Hash | Description |
|------|-------------|
| ce09522 | feat(09-01): manifest shared_files extension and summary types |
| ff68e11 | feat(09-01): scope checking logic with GlobSet |

## Next Phase Readiness

Plans 09-02 and 09-03 can proceed. All foundation types are in place:
- `check_scope()` is ready for integration into the summary builder
- `SummaryReport` and `SessionSummary` are ready to be populated from merge results
- `ManifestMeta.shared_files` feeds into scope checking via `check_scope()`
