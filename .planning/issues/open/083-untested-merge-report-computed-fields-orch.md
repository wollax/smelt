# Untested Merge Report Computed Fields in Orchestration Summary

**Phase:** 8 — Orchestration Plan & Task Graph
**Severity:** Important
**File:** `crates/smelt-cli/src/commands/orchestrate.rs` (format_orchestration_summary)

## Description

Summary output uses `merge_report.total_files_changed`, `total_insertions`, `total_deletions` without tests validating these computed fields are accurate. If `MergeRunner` populates these incorrectly or incompletely, the summary output will be wrong with no test catching the regression.
