# Smelt

## What This Is

Smelt is an orchestration layer for autonomous, spec-driven software development. It coordinates multiple AI coding agent sessions — each running in their own worktrees with Assay-enforced quality gates — and merges their outputs into cohesive, tested, verified branches and PRs. Smelt handles what happens *between* agent sessions: conflict resolution, cross-session coordination, human escalation, and forge integration.

## Core Value

Autonomous multi-session development orchestration — multiple agents work in parallel on different parts of a codebase, and Smelt merges their work into a single coherent result that passes all quality gates.

## Current Milestone: v0.1.0 Orchestration PoC

**Goal:** Prove that Smelt can coordinate multiple agent sessions in worktrees and merge their outputs into a single coherent branch with AI-assisted conflict resolution.

**Target features:**

- Coordinate 2+ agent sessions working in separate git worktrees on the same repo
- Support real agent sessions (Claude Code) and simulated/scripted sessions for development and testing
- Merge agent outputs from multiple worktrees into a single branch
- AI-assisted conflict resolution with human fallback
- Git as coordination substrate (no external database or message queue)

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] Coordinate 2+ agent sessions working in separate worktrees on the same repo
- [ ] Support real agent sessions (Claude Code) and simulated/scripted sessions
- [ ] Merge agent outputs from multiple worktrees into a single branch
- [ ] AI-assisted conflict resolution with human fallback
- [ ] Use git as the coordination substrate (no external database or message queue)

### Deferred (future milestones)

- [ ] Read Assay gate run records to make merge/reject/retry decisions
- [ ] Run verification (tests, gates) against the merged result before PR creation
- [ ] Create PRs on GitHub with structured summaries of what each agent session contributed
- [ ] Notify humans when intervention is needed (conflicts, gate failures, review requests)
- [ ] Track cost/token usage across orchestrated sessions

### Out of Scope

- Container runtime / sandboxing — Smelt orchestrates sessions, not containers. Sandboxing is a future extraction if standalone demand emerges.
- Spec authoring / gate definition — owned by Assay
- Individual agent session management — owned by Assay plugins (Claude Code, Codex, OpenCode)
- Within-session quality gates — owned by Assay's MCP server and hooks
- Web/mobile companion app — human interaction flows through forge (GitHub/ADO/GitLab) and notification channels (Slack, Teams, email via webhooks)
- Multi-machine coordination — deferred to a later milestone after single-machine orchestration is proven

## Context

### Ecosystem

Smelt is the middle layer in a three-tier stack:

| Layer | Project | Responsibility |
|-------|---------|---------------|
| Top | **Assay** (Rust, v0.2.0) | Spec-driven development, dual-track quality gates (deterministic + AI-evaluated), context management |
| Middle | **Smelt** | Multi-session orchestration, merging, conflict resolution, forge integration, human escalation |
| Bottom | (Internal to Smelt for now) | Agent session lifecycle, worktree management. Extract later if standalone demand emerges. |

### Assay Integration Surface

**Inputs Smelt reads from Assay:**
- Gate run records: `.assay/results/{spec}/{ts}-{hash}.json` — structured pass/fail with enforcement breakdown (Required vs Advisory)
- Spec definitions: `.assay/specs/{name}/` — what each session is building toward
- Team checkpoints: `.assay/checkpoints/` — agent state snapshots for recovery

**Assay's MCP tools available programmatically:**
- `gate_run(name)` — execute deterministic gates, get structured results
- `gate_report/gate_finalize` — drive agent-evaluated gates
- `spec_list/spec_get` — discover and read specs

**Boundary:** Assay notifies the agent within a session. Smelt notifies the human across sessions. Assay produces structured results; Smelt reads them and routes decisions.

### Competitive Landscape

**Axon** (axon-core/axon) — K8s-only controller for running Claude Code in ephemeral Pods. v0.4.0, Go, BSL 1.1. Single-agent-per-task, single-repo workspaces, no orchestration, no quality gates. Validates the market but targets a different user profile (K8s-native teams).

Smelt's differentiation is not "run agents in containers" (Axon does that) but "orchestrate multiple spec-driven agent sessions into coherent, verified output."

### User Tiers

| Tier | Tools | Scope |
|------|-------|-------|
| Beginner | Assay only | Single session, local, spec-driven dev |
| Power user | Assay + Smelt | Multi-branch/worktree orchestration, single machine |
| Expert | Assay + Smelt + infra | Multi-machine, K8s, fleet coordination |

### Git as Coordination Substrate

Multi-machine coordination will use git itself as the state layer — no external database, no message queue. Orchestration state, task assignments, and gate results travel through git. Projects like git-bug and git-notes validate this pattern. Specific design decisions deferred to implementation.

### Forge Support

GitHub first, then Azure DevOps, GitLab, Forgejo. Smelt's human interaction model uses forge primitives:
- Decisions needed → Issue or PR comment
- Review needed → PR with structured summary
- Status updates → forge dashboards / CI summaries
- Escalation → webhook notifications (Slack, Teams, email)

### Brainstorm Reference

Full brainstorm output from 3 explorer/challenger pairs available at `.planning/brainstorms/2026-03-09T11-35-brainstorm/SUMMARY.md`. Key insights informing this project:

- "Design for extensibility, build for today" — protocol-shaped internal interfaces without premature standardization
- "Accumulated operational intelligence" — long-term moat is the flywheel of execution history and learned patterns
- "The real competition is manual agent usage" — must be dramatically better than running agents by hand
- Graduated autonomous stewardship (suggestion dashboard → auto-execute → auto-merge) as the product vision

## Constraints

- **Assay compatibility**: Must consume Assay's existing output formats (gate run records, specs, checkpoints) without requiring Assay changes
- **Git-native**: No external database or message queue for coordination. Git is the source of truth.
- **Forge-agnostic design**: Internal abstractions must support GitHub, Azure DevOps, GitLab, and Forgejo even though GitHub ships first
- **Language**: TBD — decision deferred to milestone planning when requirements are concrete. Candidates: Rust (ecosystem alignment with Assay), .NET/C# (developer comfort), or other based on what Smelt needs (git manipulation, IPC, CLI ergonomics)

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Smelt is orchestration, not container runtime | Container runtime is an implementation detail; orchestration is the product. Extract runtime later if standalone demand emerges. | — Pending |
| Git as coordination substrate | No external dependencies. Inspectable with standard tools. Offline-capable. Proven by git-bug, git-notes. | — Pending |
| Human interaction via forge, not custom app | Developers already live in GitHub/ADO/GitLab. Building a custom UI is massive surface area with no clear advantage. | — Pending |
| Assay boundary: session-level quality, not cross-session orchestration | Assay owns specs + gates within a session. Smelt owns everything between sessions. Clean separation of concerns. | — Pending |
| Language deferred | Decision should be driven by concrete requirements (git lib quality, IPC model, CLI framework), not preference alone. | — Pending |

---
*Last updated: 2026-03-09 — Milestone v0.1.0 started*
