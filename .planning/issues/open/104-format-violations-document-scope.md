---
id: "104"
area: smelt-core
severity: suggestion
source: pr-review-phase-09
---
# Document file_scope assumption in format_violations

`format_violations()` assumes all violations in a session share the same `file_scope`. Add a documenting comment explaining this invariant and its implications for correctness.

**Files:**
- `src/analysis/report.rs`
