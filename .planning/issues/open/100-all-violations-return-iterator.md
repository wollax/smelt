---
id: "100"
area: smelt-core
severity: suggestion
source: pr-review-phase-09
---
# SummaryReport::all_violations() return iterator

`SummaryReport::all_violations()` currently returns a `Vec`. Change to return `impl Iterator` for zero-allocation iteration over violations.

**Files:**
- `src/analysis/report.rs`
