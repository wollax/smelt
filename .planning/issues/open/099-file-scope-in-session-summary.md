---
id: "099"
area: smelt-core
severity: suggestion
source: pr-review-phase-09
---
# Move file_scope to SessionSummary

`ScopeViolation::file_scope` is duplicated per violation, leading to redundant allocation. Move `file_scope` to `SessionSummary` and store a reference or index in each violation.

**Files:**
- `src/analysis/summary.rs`
- `src/analysis/mod.rs`
