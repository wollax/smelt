---
id: "102"
area: smelt-core
severity: suggestion
source: pr-review-phase-09
---
# ScopeViolation derive PartialEq

`ScopeViolation` should derive `PartialEq` to improve test assertion ergonomics and reduce boilerplate in violation comparisons.

**Files:**
- `src/analysis/summary.rs`
