# Project Milestones: Smelt

## v0.1.0 Orchestration PoC (Shipped: 2026-03-11)

**Delivered:** Proved that Smelt can coordinate multiple AI coding agent sessions in git worktrees and merge their outputs into a single coherent branch with AI-assisted conflict resolution and human fallback.

**Phases completed:** 1-10 (30 plans total)

**Key accomplishments:**

- Rust workspace with SmeltGitOps trait abstracting all git CLI operations behind async interface
- Worktree lifecycle management with orphan detection (PID liveness + staleness + git cross-reference)
- TOML session manifest with scripted sessions supporting configurable failure modes
- Sequential squash merge pipeline with file-overlap-based ordering intelligence
- AI-assisted conflict resolution with 3-way merge context, retry-with-feedback, and human fallback
- DAG-based orchestration engine with parallel dispatch, failure policies, and crash recovery
- Per-session summary with scope isolation verification via GlobSet pattern matching
- Real Claude Code agent backend with CLAUDE.md/settings injection and process group management

**Stats:**

- 199 files created/modified
- 18,158 lines of Rust
- 10 phases, 30 plans
- 3 days from project start to ship (2026-03-09 → 2026-03-11)
- 286 tests passing, 6 ignored (require Claude Code CLI)

**Git range:** `feat(01-01)` → `docs: v0.1.0 milestone audit`

**What's next:** v0.2.0 Assay Integration & Forge — read gate results, create PRs, notify humans

---
