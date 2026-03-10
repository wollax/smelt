# Phase 5: Merge Order Intelligence - Context

**Gathered:** 2026-03-10
**Status:** Ready for planning

<domain>
## Phase Boundary

Implement deterministic merge ordering that minimizes expected conflicts. Default strategy orders by session completion time; an alternative strategy analyzes file overlap between branches and merges least-overlapping pairs first. Users can preview the chosen merge order before execution.

</domain>

<decisions>
## Implementation Decisions

### Strategy selection model
- Two strategies: completion-time (default) and file-overlap (opt-in)
- Configurable in both manifest TOML and CLI flag; CLI overrides manifest
- When overlap strategy can't meaningfully differentiate (all sessions touch same files), inform the user and fall back to completion-time ordering silently
- Design as extensible enum/trait for future strategies, but only ship these two

### Overlap scoring semantics
- Unit of overlap is individual file paths (not directories or "areas")
- Binary scoring: file touched or not (no weighting by change size)
- New files count toward overlap (two sessions creating the same new file is a conflict)
- Implementation: gather per-session changed file sets via `git diff --name-only` against merge base, then compute pairwise overlap scores from those sets
- Directory-based "area" grouping deferred to future phase

### Plan output experience
- Claude's discretion on invocation style (flag vs subcommand)
- Dry-run shows full breakdown: per-session file sets, pairwise overlap scores, and final ordered list
- Human-readable table output by default
- `--json` flag for structured output (CI/scripting/agent consumption)

### Claude's Discretion
- Whether dry-run is `--dry-run` flag or `merge plan` subcommand
- Exact table formatting and column layout
- Overlap score display format (percentage, count, visual indicator)
- Tiebreaker logic when overlap scores are equal

</decisions>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches

</specifics>

<deferred>
## Deferred Ideas

- Directory/area-based overlap grouping (user-configurable "areas" like `src/Presentation`, `src/Infrastructure`) — useful for Clean Architecture projects but not all codebases organize this way
- Weighted overlap scoring (by line count or change size)
- Additional ordering strategies (minimize total diff size, dependency-aware ordering)

</deferred>

---

*Phase: 05-merge-order*
*Context gathered: 2026-03-10*
