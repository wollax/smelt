---
id: "103"
area: smelt-core
severity: suggestion
source: pr-review-phase-09
---
# Use precise error variant for glob failures

`scope.rs` uses `SmeltError::Orchestration` for glob failures. Consider creating a more precise error variant (e.g., `SmeltError::GlobError`) for better error handling and diagnostics.

**Files:**
- `src/analysis/scope.rs`
- `src/error.rs`
