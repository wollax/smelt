---
title: "Add test for exit_after = 0 edge case"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `script.rs`, there is no test for the `exit_after = 0` edge case where no steps should execute. Add a test to document the expected behavior.
