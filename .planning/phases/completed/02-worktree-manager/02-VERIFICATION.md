# Phase 02 — Worktree Manager: Verification

**Status:** gaps_found
**Score:** 18/20 must-haves verified

**Test suite:** All 53 tests pass (`cargo test --workspace`), including 10 unit tests in `smelt-core::worktree`, 12 unit tests in `smelt-core::git::cli`, and 7 CLI integration tests covering the worktree lifecycle.

---

## Plan 02-01: Core Abstractions (7/7)

| # | Must-Have | Status | Evidence |
|---|-----------|--------|----------|
| 1 | `GitOps` trait has `worktree_add` | PASS | `crates/smelt-core/src/git/mod.rs:31-36` |
| 2 | `GitOps` trait has `worktree_remove` | PASS | `crates/smelt-core/src/git/mod.rs:39-43` |
| 3 | `GitOps` trait has `worktree_list` | PASS | `crates/smelt-core/src/git/mod.rs:46` |
| 4 | `GitOps` trait has `worktree_prune`, `worktree_is_dirty`, `branch_delete`, `branch_is_merged` | PASS | `crates/smelt-core/src/git/mod.rs:49-66` |
| 5 | `GitCli` implements all new `GitOps` methods by shelling out to git | PASS | `crates/smelt-core/src/git/cli.rs:79-163` — all methods shell out via `Command::new(&self.git_binary)` |
| 6 | `WorktreeState` and `SessionStatus` types serialize/deserialize to TOML | PASS | `crates/smelt-core/src/worktree/state.rs:11-62` — derives `Serialize`/`Deserialize`, round-trip tests pass |
| 7 | `SmeltError` has `WorktreeExists`, `WorktreeNotFound`, `BranchExists`, `WorktreeDirty`, `BranchUnmerged`, `NotInitialized`, `StateDeserialization` variants | PASS | `crates/smelt-core/src/error.rs:36-61` — all seven variants present |

## Plan 02-02: Create & List (6/6)

| # | Must-Have | Status | Evidence |
|---|-----------|--------|----------|
| 8 | `smelt worktree create <name>` creates a worktree + branch in a sibling directory | PASS | CLI integration test `test_worktree_create_and_list` + `WorktreeManager::create` in `crates/smelt-core/src/worktree/mod.rs:81-165` |
| 9 | `smelt wt create <name>` short alias works | PASS | `#[command(visible_alias = "wt")]` in `main.rs:30` + integration test `test_worktree_wt_alias` |
| 10 | `smelt worktree list` shows all tracked worktrees | PASS | `execute_list` in `crates/smelt-cli/src/commands/worktree.rs:111-165` + integration test |
| 11 | Creating a worktree with a branch name that already exists produces a clear error | PASS | `WorktreeManager::create` checks `branch_exists` and returns `BranchExists` error; CLI maps it to `stderr` message + exit code 1; integration test `test_worktree_create_duplicate` |
| 12 | Creating a worktree writes `.smelt/worktrees/<name>.toml` state file | PASS | `WorktreeManager::create` saves state at line 155; unit test `create_writes_state_and_calls_git` verifies state file contents |
| 13 | `smelt init` creates the `.smelt/worktrees/` directory | PASS | `crates/smelt-core/src/init.rs:47-55` — creates `worktrees` subdirectory |

## Plan 02-03: Remove, Prune, & Integration (5/7)

| # | Must-Have | Status | Evidence |
|---|-----------|--------|----------|
| 14 | `smelt worktree remove <name>` cleans up worktree, branch, and state file | PASS | `WorktreeManager::remove` in `crates/smelt-core/src/worktree/mod.rs:245-313`; integration test `test_worktree_remove` |
| 15 | Removing a worktree with unmerged commits warns and requires `--force` | PASS | `WorktreeManager::remove` checks `branch_is_merged` and returns `BranchUnmerged` if not force; CLI prints warning at line 211-216 |
| 16 | `smelt worktree prune` cleans up orphaned worktrees | PASS | `WorktreeManager::prune` in `crates/smelt-core/src/worktree/mod.rs:356-371`; `execute_prune` in CLI |
| 17 | Orphaned worktrees detected by cross-referencing state files, git worktree list, and PID liveness | PASS | `orphan::is_likely_orphan` in `crates/smelt-core/src/worktree/orphan.rs:31-68` checks PID via `libc::kill`, staleness threshold, and git entry presence |
| 18 | Dirty worktrees prompt for confirmation before removal (unless `--yes`) | **GAP** | The `--yes` flag on `remove` skips the dialoguer prompt but then falls through to an error exit (code 1) instead of auto-confirming. When `yes=true` and the worktree is dirty (without `--force`), the command errors rather than auto-forcing. The prompt path (when `yes=false`) works correctly. See `crates/smelt-cli/src/commands/worktree.rs:187-209`. |
| 19 | Integration tests verify full create -> list -> remove lifecycle via CLI binary | PASS | `crates/smelt-cli/tests/cli_integration.rs` — tests `test_worktree_create_and_list`, `test_worktree_remove`, `test_worktree_wt_alias` cover the full lifecycle |
| 20 | Integration test for remove nonexistent, create without init | **PARTIAL** | `test_worktree_remove_nonexistent` and `test_worktree_create_without_init` exist and pass. However, there is no integration test that verifies dirty-worktree prompting or `--force` on unmerged branches (only unit tests cover those paths). |

---

## Gaps

### Gap 1: `--yes` flag on dirty worktree removal is broken (must-have #18)
**File:** `crates/smelt-cli/src/commands/worktree.rs:187-209`
**Issue:** When `--yes` is passed and the worktree is dirty (without `--force`), the code skips the prompt but then unconditionally prints the error and exits with code 1. It should auto-confirm and retry with force when `--yes` is set. The `if !yes` guard should have an `else` branch that retries with force.

### Gap 2: No integration test for dirty/force removal paths (must-have #20)
**File:** `crates/smelt-cli/tests/cli_integration.rs`
**Issue:** The integration tests cover the happy path (create, list, remove with `--force --yes`) but do not test: (a) removing a dirty worktree without `--force` to verify the error message, or (b) removing a worktree with unmerged commits. These paths are covered by unit tests in `smelt-core`, but the plan specifically calls for integration tests via the CLI binary.

---

## Success Criteria Assessment

| Criterion | Status |
|-----------|--------|
| 1. User can create a named worktree and see it with `smelt worktree list` | PASS |
| 2. User can remove a worktree and its branch is cleaned up | PASS |
| 3. Orphaned worktrees are detected and reported | PASS |
| 4. Branch name collision produces a clear error | PASS |
