---
phase: 02-worktree-manager
plan: 02
subsystem: core, cli
tags: [worktree-manager, cli-commands, state-tracking]
requires: [02-01]
provides: [WorktreeManager, worktree-create, worktree-list, wt-alias]
affects: [02-03]
tech-stack:
  added: []
  patterns: [WorktreeManager-coordinator, state-file-io, sibling-worktree-dirs]
key-files:
  created:
    - crates/smelt-cli/src/commands/worktree.rs
  modified:
    - crates/smelt-core/src/worktree/mod.rs
    - crates/smelt-core/src/worktree/state.rs
    - crates/smelt-core/src/init.rs
    - crates/smelt-core/src/lib.rs
    - crates/smelt-cli/src/main.rs
    - crates/smelt-cli/src/commands/mod.rs
decisions: []
metrics:
  duration: ~6m
  completed: 2026-03-09
---

# Phase 02 Plan 02: WorktreeManager Create & List + CLI Wiring Summary

Implemented the WorktreeManager with create and list operations and wired up the CLI subcommands (`smelt worktree create|list` with `wt` alias). State files are written to `.smelt/worktrees/` and the init command now creates the worktrees subdirectory.

## Tasks Completed

### Task 1: Implement WorktreeManager create and list
- Created `WorktreeManager<G: GitOps>` struct with `new()`, `create()`, `list()` methods
- `create()` validates .smelt/ exists, checks for duplicate session names, branch collisions, and existing paths before calling `git worktree add`
- Writes `WorktreeState` TOML to `.smelt/worktrees/<name>.toml` with relative path (`../<dir>`)
- `list()` reads all `.toml` files from worktrees dir, cross-references with `git worktree list`, returns sorted by session name
- Added `CreateWorktreeOpts` and `WorktreeInfo` types
- Added `WorktreeState::load()` and `WorktreeState::save()` helper methods
- Updated `init_project()` to create `.smelt/worktrees/` directory
- Re-exported `WorktreeManager`, `CreateWorktreeOpts`, `WorktreeInfo` from `lib.rs`
- 5 unit tests: create writes state, duplicate returns WorktreeExists, not initialized returns error, list returns created worktrees, list empty returns empty vec

### Task 2: Wire up CLI worktree create and list subcommands
- Created `commands/worktree.rs` with `WorktreeCommands` enum (Create, List variants)
- Added `Worktree` variant to `Commands` with `#[command(visible_alias = "wt")]`
- `execute_create()` prints session name, branch, and absolute path on success; handles NotInitialized, WorktreeExists, BranchExists errors
- `execute_list()` prints compact table (NAME | BRANCH | STATUS | PATH) by default, verbose adds BASE and CREATED columns
- Path display uses `std::path::absolute()` to resolve `..` segments

## Deviations

- **Clippy fix (auto):** Inlined trailing literal string in `println!` format strings to satisfy `clippy::print_literal` lint.

## Verification

- `cargo build --workspace`: clean
- `cargo test --workspace`: 32/32 passed (24 smelt-core, 8 smelt-cli integration)
- `cargo clippy --workspace -- -D warnings`: clean
- Smoke test: `smelt init && smelt worktree create test-session && smelt worktree list` produces correct output
- `.smelt/worktrees/test-session.toml` contains correct metadata
- `smelt wt list` produces same output as `smelt worktree list`
