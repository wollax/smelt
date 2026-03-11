---
id: "051"
area: smelt-core
severity: suggestion
source: pr-review-phase-06
---
# Resolve-still-has-markers re-prompt loop never exercised

No test uses a handler that returns `Resolved` while conflict markers are still present. The re-prompt loop path is untested and could regress silently.

File: `merge/mod.rs`
