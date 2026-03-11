---
id: "033"
area: smelt-core
severity: suggestion
source: pr-review-phase-04
---
# Extract shared test repo setup

`setup_test_repo()` is duplicated in `cli.rs`, `merge/mod.rs`, and `session/runner.rs` with minor variations. Extract into a shared `#[cfg(test)]` test utility module.
