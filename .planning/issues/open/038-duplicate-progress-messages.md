---
id: "038"
area: smelt-cli
severity: suggestion
source: pr-review-phase-04
---
# Progress messages duplicated between MergeRunner and CLI

MergeRunner uses `tracing::info!` for progress, CLI handler prints its own `[i/n] Merged` messages to stderr. With `SMELT_LOG=info`, user sees both. Consolidate to one source.
