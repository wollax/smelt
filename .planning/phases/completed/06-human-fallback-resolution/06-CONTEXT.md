# Phase 6: Human Fallback Resolution - Context

**Gathered:** 2026-03-10
**Status:** Ready for research

<domain>
## Phase Boundary

When a merge produces conflicts, present them to the user for manual resolution via the CLI. This is the safety net — built before AI resolution (Phase 7) so there is always a working fallback path. The user sees conflict information, edits files externally, and signals completion. Currently, conflicts cause the merge to fail and roll back entirely; this phase changes that to pause and offer resolution.

</domain>

<decisions>
## Implementation Decisions

### Conflict presentation
- Show compact conflict summary: file paths + line ranges of conflict hunks
- For small conflicts (<20 lines), show inline conflict markers in terminal
- For larger conflicts, truncate with "... N more lines" indicator
- Only show conflicted files — clean-merged files in the same session are noise
- Git's default conflict marker labels (`smelt/<session>` vs `smelt/merge/<target>`) are sufficient — no extra visual decoration needed
- `--verbose` flag on `merge run` dumps full session diff context when conflicts arise
- No editor integration (spawning `$EDITOR`) in v0.1.0 — user resolves externally

### Resolution workflow & signaling
- Flow: conflict detected → show summary → user edits files externally → presses Enter → Smelt validates → auto-stages → commits
- Validation: check that no conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`) remain in conflicted files
- If unresolved markers found: re-prompt in a loop ("N files still have conflict markers, resolve and press Enter")
- Smelt auto-stages resolved files (`git add`) after validation — no manual staging required
- Three options at conflict prompt: **[r]esolve** / **[s]kip** / **[a]bort**
  - **Resolve**: enter the edit-and-validate loop
  - **Skip**: discard this session's changes from the merge, session branch preserved (not deleted), continue with remaining sessions. Report shows session as "skipped (conflict)"
  - **Abort**: stop the merge sequence entirely (see continuation decisions below)

### Merge continuation & abort
- Show running progress tally during multi-session merge: "3 of 5 sessions merged, 1 skipped, conflict on session 4"
- On abort: ask user whether to **keep** the target branch (with successful merges so far) or **roll back** entirely
- Merge worktree preserved on abort regardless of keep/rollback choice, with notice to user about its location
- Resumability: re-running `merge run` detects existing target branch and asks "continue from where you left off, or start fresh?"
  - "Continue" checks which sessions are already merged via `git log` on target branch and skips them
  - No persistent merge-state file — git itself is the state
  - "Start fresh" deletes existing target branch and begins from scratch

### Resolution metadata
- Merge commit message records resolution method: `resolved: manual` (or `skipped` for skipped sessions)
- Template: `merge(<session>): <task-desc> [resolved: manual]` for conflict-resolved sessions
- Standard template unchanged for clean merges: `merge(<session>): <task-desc>`

### Claude's Discretion
- Exact conflict marker detection implementation (regex, line scan, etc.)
- Progress display formatting (inline updates vs. printed lines)
- How to detect already-merged sessions on resume (rev-list, log --oneline, etc.)
- Prompt styling and key handling for the resolve/skip/abort menu
- Whether skip triggers a rollback of the failed squash-merge or simply moves on

</decisions>

<specifics>
## Specific Ideas

- The resolve/skip/abort prompt should use dialoguer (already a dependency from Phase 2 for worktree removal confirmation)
- Conflict detection currently lives in `merge_squash` which checks stdout/stderr for "CONFLICT" — this can be extended to pause instead of returning an error

</specifics>

<deferred>
## Deferred Ideas

- Editor integration: `Ctrl+G` or similar to open conflicted file in `$EDITOR` at the conflict line — adds complexity around editor detection and process management
- AI-assisted resolution as first attempt before human fallback (Phase 7)
- Conflict resolution history/learning (remembering how similar conflicts were resolved before)

</deferred>

---

*Phase: 06-human-fallback-resolution*
*Context gathered: 2026-03-10*
