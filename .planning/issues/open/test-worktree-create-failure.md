---
title: "Add test for WorktreeManager::create failure behavior"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `runner.rs:44`, there is no test for `WorktreeManager::create` failure behavior, such as a branch name collision. Add a test covering this error path.
