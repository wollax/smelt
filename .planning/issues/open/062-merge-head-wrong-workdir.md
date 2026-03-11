# MERGE_HEAD ref lookup runs in repo_root instead of merge worktree

**Source:** PR #17 review round 2 (suggestion)
**Area:** smelt-core/merge
**Phase:** 7

## Description

`AiConflictHandler::handle_conflict` calls `log_subjects("target..MERGE_HEAD")` but `GitCli::run` executes in `repo_root`, not the merge worktree where `MERGE_HEAD` exists. The ref is always absent, so commit context is never available to the AI prompt. The error is silently swallowed. Fix by using `run_in(work_dir, ...)` or passing work_dir context.
