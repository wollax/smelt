---
id: "108"
area: smelt-core
severity: suggestion
source: pr-review-phase-09
---
# Enhance cli_summary JSON test with nested field verification

The `cli_summary.rs` JSON test should verify nested `SessionSummary` fields (`files`, `total_insertions`, `violations`). Currently it may only check top-level structure.

**Files:**
- `src/cli/summary.rs`
- `tests/cli_integration.rs` (or similar)
