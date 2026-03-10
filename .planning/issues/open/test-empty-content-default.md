---
title: "Add test for file with neither content nor content_file"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `script.rs`, there is no test for a file change that has neither `content` nor `content_file` set, which writes an empty string. Add a test documenting this default behavior.
