---
title: "Add test for Manifest::load with a real file"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `manifest.rs`, all tests use `parse` from a string. Add a test for `Manifest::load` that reads from an actual file on disk to cover the file I/O path.
