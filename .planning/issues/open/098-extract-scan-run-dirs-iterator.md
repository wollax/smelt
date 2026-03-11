---
id: "098"
area: smelt-core
severity: suggestion
source: pr-review-phase-09
---
# Extract shared scan_run_dirs iterator

`find_incomplete_run` and `find_latest_completed_run` in `state.rs` duplicate ~40 lines of directory scanning logic. Extract a shared `scan_run_dirs()` iterator to reduce duplication.

**Files:**
- `src/session/state.rs`
