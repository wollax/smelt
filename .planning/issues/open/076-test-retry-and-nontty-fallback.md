# No test for retry_with_feedback or AiInteractiveConflictHandler non-TTY fallback

**Source:** PR #17 review round 2 (important)
**Area:** testing
**Phase:** 7

## Description

`retry_with_feedback` has zero test coverage. The non-TTY fallback path in `AiInteractiveConflictHandler` (primary CI/non-interactive path) has no unit test. Both contain substantive logic that could regress.
