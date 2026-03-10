---
title: "Validate session name format in manifest"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `manifest.rs`, there is no validation of session name format. Names with spaces or slashes could break git branch names. Add validation to reject or sanitize invalid session names during manifest parsing.
