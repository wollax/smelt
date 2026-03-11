---
id: "034"
area: smelt-cli
severity: suggestion
source: pr-review-phase-04
---
# Integration tests should verify merged file contents

`test_merge_clean_two_sessions` verifies the target branch exists and output messages but never checks out the branch to verify file contents. Add content-level assertions.
