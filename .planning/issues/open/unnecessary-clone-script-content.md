---
title: "Remove unnecessary clone of content string in ScriptExecutor"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `script.rs:62-63`, the content string is cloned unnecessarily in `ScriptExecutor`. Could write `&**c` or restructure to avoid the allocation.
