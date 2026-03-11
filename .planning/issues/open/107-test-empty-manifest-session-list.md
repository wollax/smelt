---
id: "107"
area: smelt-core
severity: suggestion
source: pr-review-phase-09
---
# Test collect_summary with empty manifest session list

Add test for `collect_summary()` when the manifest session list is empty but session state files exist. Verify graceful handling and expected report output.

**Files:**
- `src/analysis/mod.rs`
- `tests/analysis_integration.rs` (or similar)
