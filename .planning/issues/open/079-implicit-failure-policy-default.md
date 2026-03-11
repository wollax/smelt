# Implicit Failure Policy Default

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Important
**File:** `crates/smelt-core/src/session/manifest.rs`, `crates/smelt-core/src/orchestrate/executor.rs`

## Description

`ManifestMeta.on_failure` is `Option<FailurePolicy>` with no serde default. When omitted from manifest, executor uses `unwrap_or_default()` which silently becomes `SkipDependents`. The implicit fallback is undocumented and users may not realize sessions continue after failures unless they explicitly set `on_failure = "abort"`.
