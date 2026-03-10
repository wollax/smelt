---
title: "Clarify partial failure behavior for single-file steps"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `script.rs:42-48`, partial failure with a single-file step behaves unexpectedly: `(1/2).max(1) = 1` writes ALL files, making partial failure indistinguishable from normal execution. Clarify the intended behavior and add a test documenting it.
