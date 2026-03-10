---
title: "Use TaskSource enum for task/task_file XOR constraint"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `manifest.rs:33-47`, the `task`/`task_file` XOR constraint is enforced at runtime, not in the type system. Consider replacing the two `Option` fields with a `TaskSource` enum (e.g., `Inline(String)` / `File(PathBuf)`) to make invalid states unrepresentable.
