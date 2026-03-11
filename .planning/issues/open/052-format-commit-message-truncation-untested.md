---
id: "052"
area: smelt-core
severity: suggestion
source: pr-review-phase-06
---
# format_commit_message truncation untested

The `floor_char_boundary` truncation for long descriptions has no unit test. Off-by-one errors in the boundary calculation could silently corrupt commit messages.

File: `merge/mod.rs`
