---
id: "036"
area: smelt-core
severity: suggestion
source: pr-review-phase-04
---
# CompletedSession could hold WorktreeState directly

`CompletedSession` clones individual fields from `WorktreeState`. Hold the state directly to reduce intermediate types and keep data in one place.
