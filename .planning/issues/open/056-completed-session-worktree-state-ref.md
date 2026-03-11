# CompletedSession should reference WorktreeState

**Source:** PR #17 review (suggestion)
**Area:** smelt-core/merge
**Phase:** 5

## Description

`CompletedSession` duplicates fields from `WorktreeState` (session name, branch name, changed files). Consider holding a reference or borrowing from the original `WorktreeState` to avoid data duplication and potential inconsistency.
