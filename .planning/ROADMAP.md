# Roadmap — Smelt

Smelt is a multi-agent orchestration layer that coordinates AI coding sessions in git worktrees and merges their outputs into coherent, verified branches. The v0.1.0 milestone proves the core loop: create worktrees, run agents (real and scripted), merge with AI-assisted conflict resolution, and report results — all using git as the coordination substrate.

## Milestones

| # | Milestone | Status |
|---|-----------|--------|
| 1 | v0.1.0 Orchestration PoC | 🔄 In Progress |
| 2 | v0.2.0 Assay Integration & Forge | ○ Planned |
| 3 | v0.3.0 Multi-Machine Coordination | ○ Planned |

---

## v0.1.0 — Orchestration PoC

**Goal:** Prove that Smelt can coordinate multiple agent sessions in worktrees and merge their outputs into a single coherent branch with AI-assisted conflict resolution and human fallback.

### Phase 1: Project Bootstrap & Git Operations Layer

**Goal:** Establish the Rust project skeleton, CLI entry point, and the foundational `SmeltGitOps` trait that wraps git CLI operations. All subsequent components depend on this layer. Git-native state storage begins here — `.smelt/` directory structure and serialization conventions.

**Dependencies:** None (first phase)

**Requirements:** ORCH-01

**Success Criteria:**
1. User can run `smelt --version` and `smelt --help` and get meaningful output
2. `.smelt/` directory is created in the repo root on first run, storing orchestration state as git-trackable files (no external database)
3. Git operations (branch create, status, rev-parse) execute correctly via the trait abstraction and produce structured results
4. CI pipeline runs `cargo build`, `cargo test`, `cargo clippy` on every push

**Plans:**

| Plan | Wave | Title | Tasks |
|------|------|-------|-------|
| 01-01 | 1 | Workspace skeleton & CI pipeline | Rust workspace with smelt-cli + smelt-core crates; GitHub Actions CI |
| 01-02 | 2 | Core library: errors, GitOps, init | SmeltError types, GitOps trait, GitCli impl, preflight(), init_project(), unit tests |
| 01-03 | 3 | CLI binary & integration tests | Clap CLI with init command, context-aware no-args, --no-color, integration tests |

### Phase 2: Worktree Manager

**Goal:** Implement worktree lifecycle management — create, track, list, remove, and cleanup worktrees for agent sessions. Address critical pitfalls: orphaned worktree detection, branch collision prevention, and HEAD/index isolation (always specify worktree path in git commands).

**Dependencies:** Phase 1 (git operations layer)

**Requirements:** SESS-01

**Success Criteria:**
1. User can create a named worktree for a session and see it listed with `smelt worktree list`
2. User can remove a worktree and its associated branch is cleaned up
3. On startup, orphaned worktrees (from prior crashes) are detected and reported to the user
4. Attempting to create a worktree with a branch name that already exists produces a clear error (no silent corruption)

**Plans:**

| Plan | Wave | Title | Tasks |
|------|------|-------|-------|
| 02-01 | 1 | Foundation: deps, errors, GitOps extension, domain types | Error variants, WorktreeState/SessionStatus types, GitOps trait worktree methods, GitCli impl |
| 02-02 | 2 | WorktreeManager create/list + CLI wiring | WorktreeManager struct, create/list operations, CLI subcommands with wt alias |
| 02-03 | 3 | Remove, prune, orphan detection + integration tests | WorktreeManager remove/prune, orphan detection, dialoguer prompts, CLI wiring, integration tests |

### Phase 3: Session Manifest & Scripted Sessions

**Goal:** Define the session manifest format (what each agent should work on) and implement the scripted/simulated session backend. Scripted sessions are the primary testing mechanism for the entire merge pipeline — they must support configurable behaviors: commit count, file patterns, deliberate conflict generation, and failure modes.

**Dependencies:** Phase 2 (worktree manager)

**Requirements:** SESS-02, SESS-04

**Success Criteria:**
1. User can define a session manifest (TOML/JSON) specifying 2+ sessions with task descriptions and worktree configuration
2. User can launch a scripted session that creates commits in its worktree according to a script definition
3. Scripted sessions can be configured to produce merge conflicts (overlapping file edits) for testing the merge pipeline
4. Session completion is detectable by the orchestrator (exit code, marker file, or branch state)
5. Process group management ensures scripted session processes are cleaned up on orchestrator crash (no zombies)

### Phase 4: Sequential Merge

**Goal:** Implement the core merge operation — take outputs from multiple agent worktrees and merge them sequentially into a single target branch. Clean merges (no conflicts) work end-to-end. This is the central value proposition of Smelt.

**Dependencies:** Phase 3 (scripted sessions provide test inputs)

**Requirements:** MERGE-01

**Success Criteria:**
1. User can run `smelt merge` and have 2+ worktree branches merged sequentially into a target branch
2. When all merges are clean (no conflicts), the target branch contains the combined work from all sessions
3. Each merge step is atomic — if an intermediate merge fails, the target branch is not left in a corrupted state
4. Merge operations are serialized (no concurrent merges that could corrupt the index)

**Plans:**

| Plan | Wave | Title | Tasks |
|------|------|-------|-------|
| 04-01 | 1 | Foundation: error variants + GitOps merge methods | Merge error variants (MergeConflict, MergeTargetExists, NoCompletedSessions), 7 new GitOps methods (merge_base, merge_squash, branch_create, diff_numstat, unmerged_files, reset_hard, rev_parse) + GitCli implementations |
| 04-02 | 2 | Core: MergeRunner + merge pipeline | MergeRunner struct, merge types (MergeReport, DiffStat), sequential squash merge loop, rollback on conflict, cleanup on success, session filtering, worktree_add_existing |
| 04-03 | 3 | CLI + Integration: smelt merge command + e2e tests | CLI merge command with progress/summary output, --target flag, integration tests for clean merge, conflict rollback, edge cases |

### Phase 5: Merge Order Intelligence

**Goal:** Implement deterministic merge ordering that minimizes expected conflicts. The default strategy orders by session completion time; an alternative strategy analyzes file overlap between branches and merges least-overlapping pairs first.

**Dependencies:** Phase 4 (sequential merge)

**Requirements:** MERGE-04

**Success Criteria:**
1. Merge order is deterministic given the same set of completed sessions (not random or racy)
2. User can see the chosen merge order before execution (dry-run or plan output)
3. File-overlap-based ordering produces fewer conflicts than naive ordering when tested with scripted sessions that have known overlap patterns

**Plans:**

| Plan | Wave | Title | Tasks |
|------|------|-------|-------|
| 05-01 | 1 | Foundation: MergeOrderStrategy enum, diff_name_only, deps | MergeOrderStrategy enum + MergeOpts/ManifestMeta fields, diff_name_only GitOps method + GitCli impl, comfy-table + serde_json workspace deps |
| 05-02 | 2 | Core: Ordering algorithms + MergeRunner integration | ordering.rs with greedy file-overlap algorithm, collect_sessions populates changed_files, strategy resolution (CLI > manifest > default), MergePlan type |
| 05-03 | 3 | CLI: merge run/plan subcommands + table/JSON output | Restructure CLI to merge run\|plan, comfy-table plan display, --json flag, --strategy flag, MergeRunner.plan() dry-run method, integration tests |

### Phase 6: Human Fallback Resolution

**Goal:** When a merge produces conflicts, present them to the user for manual resolution via the CLI. This is the safety net — built before AI resolution so there is always a working fallback path. The user sees conflict markers, edits files, and confirms resolution.

**Dependencies:** Phase 4 (sequential merge produces conflicts to resolve)

**Requirements:** MERGE-03

**Success Criteria:**
1. When a merge conflict occurs, the user is prompted with the conflicting files and conflict context
2. User can open the conflicting files, resolve manually, and signal completion to Smelt
3. After manual resolution, the merge continues (or the user can abort the entire merge sequence)
4. The resolution is recorded in the merge commit message (who resolved, method: manual)

**Plans:**

| Plan | Wave | Title | Tasks |
|------|------|-------|-------|
| 06-01 | 1 | Foundation: conflict types, marker scanning, log_subjects | ConflictAction/ResolutionMethod enums, ConflictScan/ConflictHunk structs, scan_conflict_markers(), log_subjects GitOps method, MergeOpts/MergeReport extensions |
| 06-02 | 2 | Core: ConflictHandler trait + merge loop refactor | ConflictHandler trait, NoopConflictHandler, refactored merge_sessions with resolve/skip/abort flow, resume detection, MergeAborted error |
| 06-03 | 3 | CLI: InteractiveConflictHandler + integration tests | Interactive handler with dialoguer Select, --verbose flag, conflict summary display, resolution status output, integration tests for all three conflict action paths |

### Phase 7: AI Conflict Resolution

**Goal:** Add AI-assisted conflict resolution as the first attempt before human fallback. The resolver sends conflict context (markers, surrounding code, session descriptions) to an LLM, applies the proposed resolution, and asks for user confirmation. If the AI resolution is rejected or fails, the human fallback from Phase 6 activates.

**Dependencies:** Phase 6 (human fallback as safety net)

**Requirements:** MERGE-02

**Success Criteria:**
1. When a merge conflict occurs, AI resolution is attempted first (before prompting for manual resolution)
2. The proposed AI resolution is shown to the user with a diff, and the user can accept or reject it
3. Rejected AI resolutions fall back to the human manual resolution flow from Phase 6
4. Resolution metadata (method: ai-assisted, model used, user-accepted) is recorded in the merge commit

**Plans:**

| Plan | Wave | Title | Tasks |
|------|------|-------|-------|
| 07-01 | 1 | Foundation: AiProvider trait, GenAiProvider, AiConfig, prompt templates | AiProvider trait + GenAiProvider backed by genai, AiConfig for config.toml, prompt construction, SmeltError::AiResolution |
| 07-02 | 2 | Core: AiConflictHandler, show_index_stage, ResolutionMethod variants | AiConflictHandler implementing ConflictHandler, 3-way context extraction via git index stages, AiAssisted/AiEdited resolution methods, format_commit_message update |
| 07-03 | 3 | CLI: AI resolution UX, diff display, fallback chain, --no-ai flag | Accept/Edit/Reject flow with colored diffs via similar, retry-with-feedback, manual fallback, --no-ai flag, ConflictAction carries ResolutionMethod, integration tests |

### Phase 8: Orchestration Plan & Task Graph

**Goal:** Enable the user to define a complete orchestration plan — a task graph specifying which sessions to run, their dependencies, and how to merge the results. The orchestrator executes the plan: creates worktrees, launches sessions, waits for completion, merges in order, resolves conflicts, and reports results.

**Dependencies:** Phases 1-7 (all components available)

**Requirements:** ORCH-04

**Success Criteria:**
1. User can define an orchestration plan (TOML/JSON) with session tasks and dependency edges
2. Independent sessions run in parallel; dependent sessions wait for prerequisites
3. The orchestrator drives the full lifecycle: plan → create worktrees → launch sessions → wait → merge → resolve → report
4. Plan execution can be interrupted and resumed (crash recovery via git-native state)

**Plans:**

| Plan | Wave | Title | Tasks |
|------|------|-------|-------|
| 08-01 | 1 | Foundation: deps, manifest extensions, DAG builder, orchestration types | Workspace deps (petgraph, indicatif, tokio-util), manifest depends_on/parallel_by_default/on_failure with validation, orchestrate module with types (FailurePolicy, RunState, SessionRunState) and DAG builder (build_dag, ready_set, mark_skipped_dependents) |
| 08-02 | 2 | Core: State persistence + Orchestrator execution engine | RunStateManager for .smelt/runs/ persistence, Orchestrator struct with run()/resume() using JoinSet for parallel dispatch, failure policy enforcement, merge phase integration |
| 08-03 | 3 | CLI: `smelt orchestrate run` + dashboard + integration tests | CLI command with indicatif dashboard, comfy-table summary, --json output, Ctrl-C handling, resume detection, integration tests for parallel/sequential/diamond/failure scenarios |

### Phase 9: Session Summary & Scope Isolation

**Goal:** After sessions complete and merge, provide a structured summary of what each agent contributed (files changed, lines added/removed, commit messages). Verify that agents stayed within their assigned scope — flag sessions that modified files outside their task description.

**Dependencies:** Phase 8 (orchestrator completes sessions and merges)

**Requirements:** ORCH-02, ORCH-03

**Success Criteria:**
1. After merge, user sees a per-session summary showing files changed, lines added/removed, and commit messages
2. Sessions that modified files outside their assigned scope are flagged with a warning
3. Scope violations do not block the merge but are prominently reported (advisory, not enforcement)
4. Summary output is structured (JSON) and human-readable (terminal formatted)

### Phase 10: Real Agent Sessions

**Goal:** Add Claude Code as a real agent backend for the session controller. A real agent session launches Claude Code in a worktree with a task prompt derived from the session manifest. This is the final piece — slotting into the proven interface that scripted sessions validated across Phases 3-9.

**Dependencies:** Phase 8 (orchestrator lifecycle), Phase 3 (session controller interface)

**Requirements:** SESS-03

**Success Criteria:**
1. User can launch a real Claude Code session in a worktree via the orchestration plan
2. The agent receives its task description from the session manifest and works within its assigned worktree
3. Agent process lifecycle is managed correctly — graceful shutdown on orchestrator interrupt, zombie prevention via process group management
4. End-to-end: an orchestration plan with 2+ real agent sessions produces a merged branch with combined work

**Plans:**

| Plan | Wave | Title | Tasks |
|------|------|-------|-------|
| 10-01 | 1 | AgentExecutor module: spawn Claude Code, CLAUDE.md/settings injection, process lifecycle | AgentExecutor struct with execute(), prompt construction, CLAUDE.md + settings.json injection, timeout/cancel via tokio::select!, unit tests |
| 10-02 | 2 | Orchestrator + SessionRunner integration, CLI preflight | Wire AgentExecutor into orchestrator dispatch (script=None branch), preflight `claude` binary check, SessionRunner dispatch, CLI startup message |
| 10-03 | 3 | Integration tests + end-to-end verification | Agent executor integration tests (#[ignore] for CI), orchestrator e2e with 2 agent sessions, timeout/cancel tests, example manifest, manual verification |

---

## Progress Summary

| Phase | Name | Requirements | Status |
|-------|------|-------------|--------|
| 1 | Project Bootstrap & Git Operations Layer | ORCH-01 | Complete |
| 2 | Worktree Manager | SESS-01 | Complete |
| 3 | Session Manifest & Scripted Sessions | SESS-02, SESS-04 | Complete |
| 4 | Sequential Merge | MERGE-01 | Complete |
| 5 | Merge Order Intelligence | MERGE-04 | Complete |
| 6 | Human Fallback Resolution | MERGE-03 | Complete |
| 7 | AI Conflict Resolution | MERGE-02 | Complete |
| 8 | Orchestration Plan & Task Graph | ORCH-04 | Complete |
| 9 | Session Summary & Scope Isolation | ORCH-02, ORCH-03 | Complete |
| 10 | Real Agent Sessions | SESS-03 | Pending |
