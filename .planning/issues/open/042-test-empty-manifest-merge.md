---
id: "042"
area: smelt-core
severity: suggestion
source: pr-review-phase-04
---
# No test for empty manifest (zero sessions) merge

If manifest has zero sessions, `collect_sessions` returns empty vec triggering `NoCompletedSessions`. Subtly different from "all sessions failed" — deserves its own test.
