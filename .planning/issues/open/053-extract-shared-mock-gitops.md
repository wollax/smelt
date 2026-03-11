# Extract shared mock GitOps to test utility

**Source:** PR #17 review (suggestion)
**Area:** testing
**Phase:** 7

## Description

`MockGitOps` is now duplicated in `ai_handler.rs` tests. Extract it to a shared test utility module (e.g., `smelt-core/src/test_support.rs` behind `#[cfg(test)]`) so all test modules can reuse it without duplication.

## Context

The mock was added in the Phase 7 PR review fix commit. Other test files (e.g., merge integration tests) could benefit from the same mock.
