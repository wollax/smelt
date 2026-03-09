# Plan 01-02 Summary: Core Library (Error, GitOps, Init)

**Phase:** 01-project-bootstrap-git-ops
**Plan:** 02
**Status:** Complete
**Duration:** ~20 minutes (18:38Z – 19:58Z)

---

## Tasks Completed

| # | Title | Commit | Status |
|---|-------|--------|--------|
| 1 | Implement error types, GitOps trait, and GitCli | `a54d294` | Done |
| 2 | Implement init_project and unit tests | `62af701` | Done |

## Artifacts Created

| File | Purpose |
|------|---------|
| `crates/smelt-core/src/error.rs` | `SmeltError` enum (5 variants) with `thiserror`, `Result<T>` alias, `SmeltError::io()` convenience constructor |
| `crates/smelt-core/src/git/mod.rs` | `GitOps` trait (4 async methods), `preflight()` synchronous git discovery |
| `crates/smelt-core/src/git/cli.rs` | `GitCli` struct implementing `GitOps` via `tokio::process::Command` |
| `crates/smelt-core/src/init.rs` | `init_project()` creating `.smelt/config.toml` with cleanup on failure |
| `crates/smelt-core/src/lib.rs` | Module declarations and public re-exports |

## Key Links Established

- `GitCli` implements `GitOps` trait
- `preflight()` returns `(PathBuf, PathBuf)` used to construct `GitCli`
- `init_project()` uses `repo_root` from preflight to place `.smelt/`
- All functions return `SmeltError` variants on failure

## Test Coverage

7 unit tests, all passing:

- `test_preflight_succeeds_in_git_repo` — validates git binary + repo discovery
- `test_repo_root` — GitCli returns correct repo root
- `test_current_branch` — GitCli returns main/master
- `test_head_short` — GitCli returns valid short hash
- `test_is_inside_work_tree` — GitCli detects work tree
- `test_init_creates_smelt_dir` — creates `.smelt/config.toml` with `version = 1`
- `test_init_already_initialized` — returns `AlreadyInitialized` on re-init

## Verification

- `cargo test -p smelt-core` — 7/7 passed
- `cargo clippy -p smelt-core -- -D warnings` — clean

## Deviations

None. Plan executed as written.

## Design Notes

- `GitOps` trait uses native `async fn` in traits (Rust 1.93+ RPITIT). No `async-trait` or `trait_variant` crates needed — methods return `impl Future<Output = Result<T>> + Send`.
- `preflight()` is synchronous (`std::process::Command`) as specified — runs before tokio runtime engagement.
- `init_project` cleanup is best-effort: if `config.toml` write fails, `.smelt/` is removed; cleanup errors are silently ignored.

---

*Completed: 2026-03-09*
