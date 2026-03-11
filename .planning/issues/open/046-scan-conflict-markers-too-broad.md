---
id: "046"
area: smelt-core
severity: suggestion
source: pr-review-phase-06
---
# scan_conflict_markers matches ======= too broadly

`starts_with("=======")` also matches Markdown horizontal rules and other separator patterns. Use `trimmed == "======="` or explicitly exclude lines with extra `=` characters to avoid false positives.

File: `conflict.rs`
