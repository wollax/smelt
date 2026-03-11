---
id: "106"
area: smelt-core
severity: suggestion
source: pr-review-phase-09
---
# Test collect_summary with partial git operation failures

Add test for `collect_summary()` when git operations partially fail (e.g., `diff_numstat` succeeds but `diff_name_only` fails). Verify error handling and session state consistency.

**Files:**
- `src/analysis/mod.rs`
- `tests/analysis_integration.rs` (or similar)
