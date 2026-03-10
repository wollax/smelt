---
title: "Add non-unix stub for process kill_group"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `process.rs`, `#[cfg(unix)]` is only on `kill_group`. The struct compiles on all platforms but cannot be signaled on non-unix. Consider adding a `#[cfg(not(unix))]` stub or a platform abstraction to prevent silent compilation with missing functionality.
