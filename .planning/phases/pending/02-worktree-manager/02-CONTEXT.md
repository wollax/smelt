# Phase 2: Worktree Manager - Context

**Gathered:** 2026-03-09
**Status:** Ready for planning

<domain>
## Phase Boundary

Implement worktree lifecycle management for agent sessions — create, track, list, remove, and cleanup worktrees. Address orphaned worktree detection, branch collision prevention, and HEAD/index isolation. This phase does NOT include session execution, manifest parsing, or merge operations.

</domain>

<decisions>
## Implementation Decisions

### Naming & location
- Hybrid naming: auto-generated from session name/ID by default, with optional `--name` override for manual use
- Default worktree location: sibling to the main repo (e.g., `../myrepo-smelt-session-name/`)
- Location is configurable via `.smelt/config` override
- Branch name prefix convention: Claude's discretion

### Orphan handling
- On startup, prompt the user to clean up detected orphaned worktrees (interactive prompt)
- Orphan detection uses both signals: Smelt state tracking (session process gone) AND git-native worktree cross-reference
- Safety check for uncommitted changes before cleanup: Claude's discretion (but must prevent data loss)

### Branch cleanup on removal
- When removing a worktree, delete its associated branch
- If the branch has unmerged commits, warn and require `--force` to proceed

### State tracking
- One TOML file per worktree stored in `.smelt/` (e.g., `.smelt/worktrees/session-name.toml`)
- Metadata per worktree: session name, branch name, worktree path, creation timestamp, base commit/branch, session status (created/running/completed/failed/orphaned), last-updated timestamp, PID of session process, exit code, task description, assigned file scope
- State files are committed to git (aligns with ORCH-01 git-native state)
- Worktree creation supports `--base <branch|commit>` flag, defaults to HEAD

### CLI surface
- Full subcommand: `smelt worktree create|list|remove|prune`
- Short alias: `smelt wt` (same subcommands)
- Destructive operations (remove, prune) prompt for confirmation only if the worktree has uncommitted changes; clean worktrees removed without asking
- `--yes` / `-y` flag to skip confirmation (for scripts/CI)

### Claude's Discretion
- Branch name prefix convention (e.g., `smelt/` prefix or bare names)
- `smelt worktree list` output format (table vs compact, verbosity levels)
- `smelt worktree create` stdout behavior (path-only vs structured summary)
- Safety check behavior for uncommitted changes during orphan cleanup (warn+force, auto-stash, etc.)

</decisions>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 02-worktree-manager*
*Context gathered: 2026-03-09*
