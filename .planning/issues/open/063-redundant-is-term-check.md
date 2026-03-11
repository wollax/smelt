# Redundant is_term() check in build_conflict_handler

**Source:** PR #17 review round 2 (suggestion)
**Area:** smelt-cli/merge
**Phase:** 7

## Description

`build_conflict_handler` checks `!is_term()` at the top to skip AI, and `AiInteractiveConflictHandler::handle_conflict` checks it again as a safety net. The outer check is intentional early-exit but could use a brief comment clarifying it's not redundant.
