# FNV-1a Collision Risk for Resume Validation

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Important
**File:** `crates/smelt-core/src/orchestrate/state.rs` (compute_manifest_hash)

## Description

Resume validates manifest hash using FNV-1a, which is not collision-resistant. Two different manifests could theoretically hash to the same 64-bit value, causing resume to proceed with a changed manifest. Consider using SHA-256 or a wider hash for correctness-critical validation. Low probability but high impact if triggered.
