---
title: "Use ContentSource enum for content/content_file XOR constraint"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `manifest.rs:87-93`, the same `Option`/`Option` problem exists for `content`/`content_file` in `FileChange`. Consider replacing with a `ContentSource` enum (e.g., `Inline(String)` / `File(PathBuf)`) to make invalid states unrepresentable at the type level.
