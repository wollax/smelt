---
title: "Remove unnecessary clones of base_ref strings in runner"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `runner.rs:46-49`, `base_ref` strings are cloned unnecessarily. Consider borrowing if `CreateWorktreeOpts` can accept `&str` or `Cow<str>`.
