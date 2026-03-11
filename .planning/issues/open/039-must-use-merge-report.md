---
id: "039"
area: smelt-core
severity: suggestion
source: pr-review-phase-04
---
# Add #[must_use] to MergeReport and MergeSessionResult

These structs carry important result data. Adding `#[must_use]` prevents accidental discard.
