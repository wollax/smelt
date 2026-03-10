---
title: "Extract shared test helper setup_test_repo"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `runner.rs` and `script.rs` tests, there is test helper duplication. Extract `setup_test_repo` into a shared `#[cfg(test)] mod test_utils` to reduce duplication.
