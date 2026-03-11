---
id: "047"
area: smelt-core
severity: suggestion
source: pr-review-phase-06
---
# No "still has markers" message on re-prompt

When the user chooses "Resolved" but conflict markers are still present, the loop silently re-prompts without explaining why. Print a message indicating that markers were detected so the user understands the re-prompt.

File: `merge/mod.rs`
