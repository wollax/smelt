---
id: "105"
area: smelt-core
severity: suggestion
source: pr-review-phase-09
---
# Add diagnostic logging for missing sessions

`analysis.rs` `collect_summary()` skips sessions missing from `session_states` silently. Add `warn!()` logging to diagnose gaps or inconsistencies in session data.

**Files:**
- `src/analysis/mod.rs`
