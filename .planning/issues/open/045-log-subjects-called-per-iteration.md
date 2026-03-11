---
id: "045"
area: smelt-core
severity: suggestion
source: pr-review-phase-06
---
# log_subjects called per-iteration in merge loop

If resume detection is re-added, `log_subjects` is fetched on every loop iteration. Fetch subjects once before the loop to avoid redundant work.

File: `merge/mod.rs`
