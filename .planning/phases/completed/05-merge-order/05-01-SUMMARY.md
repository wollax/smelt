---
phase: 05-merge-order
plan: 01
subsystem: merge-types
tags: [merge-order, strategy-enum, git-ops, serde]
dependency-graph:
  requires: [phase-04]
  provides: [MergeOrderStrategy, diff_name_only, comfy-table-dep, serde_json-dep]
  affects: [05-02, 05-03]
tech-stack:
  added: [comfy-table v7, serde_json v1]
  patterns: [non_exhaustive enum, serde rename_all kebab-case]
key-files:
  created: []
  modified:
    - Cargo.toml
    - crates/smelt-core/Cargo.toml
    - crates/smelt-cli/Cargo.toml
    - crates/smelt-core/src/merge/types.rs
    - crates/smelt-core/src/merge/mod.rs
    - crates/smelt-core/src/session/manifest.rs
    - crates/smelt-core/src/session/runner.rs
    - crates/smelt-core/src/git/mod.rs
    - crates/smelt-core/src/git/cli.rs
    - crates/smelt-core/src/lib.rs
decisions:
  - MergeOrderStrategy uses serde rename_all kebab-case for TOML/JSON serialization
  - Serialize derive added to DiffStat, MergeSessionResult, MergeReport for JSON output in Plan 03
  - ManifestMeta.merge_strategy is Option — backward-compatible with existing manifests
metrics:
  duration: ~6 minutes
  completed: 2026-03-10
---

# Phase 05 Plan 01: Types, Trait Methods & Dependencies Summary

MergeOrderStrategy enum with CompletionTime/FileOverlap variants, diff_name_only GitOps method, and comfy-table + serde_json workspace dependencies wired up.

## Tasks Completed

### Task 1: Add MergeOrderStrategy enum and extend MergeOpts + ManifestMeta
- Added `MergeOrderStrategy` enum with `#[non_exhaustive]`, `#[serde(rename_all = "kebab-case")]`, `Default` (CompletionTime)
- Extended `MergeOpts` with `strategy: Option<MergeOrderStrategy>` and `with_strategy()` constructor
- Extended `ManifestMeta` with `merge_strategy: Option<MergeOrderStrategy>` (backward-compatible)
- Added `Serialize` derive to `DiffStat`, `MergeSessionResult`, `MergeReport`
- Re-exported `MergeOrderStrategy` from `merge::mod` and `lib.rs`
- Updated all struct literal constructions of `ManifestMeta` (manifest.rs tests, merge/mod.rs tests, session/runner.rs tests)

### Task 2: Add diff_name_only to GitOps trait + GitCli impl + workspace deps
- Added `diff_name_only(base_ref, head_ref) -> Vec<String>` to `GitOps` trait
- Implemented in `GitCli` using `git diff --name-only`
- Added `comfy-table = "7"` and `serde_json = "1"` to workspace dependencies
- Wired `serde_json` into smelt-core, `comfy-table` + `serde_json` into smelt-cli
- Added 2 tests: `test_diff_name_only` (2-file branch diff) and `test_diff_name_only_empty` (same-ref)

## Verification Results

- `cargo build --workspace` — clean
- `cargo test --workspace` — 92 tests pass (90 existing + 2 new)
- `cargo clippy --workspace -- -D warnings` — clean
- `MergeOrderStrategy::default()` = `CompletionTime` (via `#[default]` attribute)
- `MergeOpts::default()` has `strategy: None`, `target_branch: None`
- Existing manifest TOML without `merge_strategy` parses successfully (backward-compatible)

## Deviations

- **[Rule 3 - Blocking]** Updated `ManifestMeta` struct literal in `session/runner.rs` tests — required to compile after adding the new field. Not listed in plan files_modified but necessary.

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| `serde(rename_all = "kebab-case")` on MergeOrderStrategy | Consistent with TOML/CLI conventions (e.g. `completion-time`, `file-overlap`) |
| Serialize on DiffStat/MergeSessionResult/MergeReport | Pre-wired for Plan 03 JSON output — avoids a separate derive-only commit later |
| `Option<MergeOrderStrategy>` (not bare enum) in ManifestMeta and MergeOpts | None = use default; Some = explicit override. Allows distinguishing "not set" from "set to default" |

## Next Phase Readiness

Plan 02 can proceed immediately — `MergeOrderStrategy` enum and `diff_name_only` method are available. Plan 03 can proceed — `comfy-table` and `serde_json` dependencies plus `Serialize` derives are in place.
