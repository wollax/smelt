---
title: "Add test for exit_after larger than step count"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `script.rs`, there is no test for when `exit_after` is larger than the total step count. Add a test to document that all steps execute normally in this case.
