# Extract shared test repo setup with consistent git isolation

**Area:** tests
**Priority:** Low
**Source:** PR review (Phase 10)

The CLI integration test's `setup_test_repo` sets `GIT_CONFIG_NOSYSTEM=1` and provides `HOME` override, but the unit test helper in `runner.rs` does not. On systems with unusual global git configuration the `runner.rs` tests can fail spuriously. Consider extracting a shared test utility.

**Files:** `crates/smelt-cli/tests/cli_agent.rs:13`, `crates/smelt-core/src/session/runner.rs:259`
