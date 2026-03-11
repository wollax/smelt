---
id: "101"
area: smelt-core
severity: suggestion
source: pr-review-phase-09
---
# SessionSummary::files_changed() trivial method

`SessionSummary::files_changed()` is trivial (returns `files.len()`). Either make `files` field public to allow direct access, or remove the method entirely.

**Files:**
- `src/analysis/summary.rs`
