---
id: "013"
area: smelt-core
severity: suggestion
source: pr-review-phase-02
---

# Consider thiserror for cleaner error Display impls

SmeltError already uses `thiserror::Error` derive, but some variants have long format strings inline. Consider whether the Display impls could be cleaner or if additional context methods would help.

**File:** `crates/smelt-core/src/error.rs`
