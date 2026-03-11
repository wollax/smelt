# Smelt

## What This Is

Smelt is an orchestration layer for autonomous, multi-agent software development. It coordinates multiple AI coding agent sessions — each running in their own git worktrees — and merges their outputs into a single coherent branch with AI-assisted conflict resolution and human fallback. Smelt handles what happens *between* agent sessions: worktree management, merge orchestration, conflict resolution, scope verification, and session summarization.

## Core Value

Autonomous multi-session development orchestration — multiple agents work in parallel on different parts of a codebase, and Smelt merges their work into a single coherent result.

## Current State

Shipped v0.1.0 Orchestration PoC with 18,158 lines of Rust across 2 crates (smelt-cli + smelt-core).

**Tech stack:** Rust (Edition 2024), tokio async runtime, clap CLI, genai for LLM provider abstraction, petgraph for DAG scheduling, indicatif + comfy-table for terminal UX.

**Capabilities shipped:**
- Coordinate 2+ agent sessions (real Claude Code + scripted) in separate git worktrees
- Sequential squash merge with file-overlap-based ordering intelligence
- AI-assisted conflict resolution (Anthropic, OpenAI, Ollama, Gemini) with human fallback
- DAG-based task graph with parallel dispatch, failure policies, crash recovery
- Per-session summary with scope isolation verification
- Git-native orchestration state (.smelt/ directory)
- 286 tests passing (6 ignored — require Claude Code CLI)

## Requirements

### Validated

- Coordinate 2+ agent sessions working in separate worktrees on the same repo — v0.1.0
- Support real agent sessions (Claude Code) and simulated/scripted sessions — v0.1.0
- Merge agent outputs from multiple worktrees into a single branch — v0.1.0
- AI-assisted conflict resolution with human fallback — v0.1.0
- Use git as the coordination substrate (no external database or message queue) — v0.1.0

### Active

(None — define requirements for next milestone via `/kata-add-milestone`)

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
| Middle | **Smelt** (Rust, v0.1.0) | Multi-session orchestration, merging, conflict resolution, forge integration, human escalation |
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
- **Language**: Rust — decided in v0.1.0, driven by ecosystem alignment with Assay, git CLI manipulation, async runtime (tokio), and single-binary distribution

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Smelt is orchestration, not container runtime | Container runtime is an implementation detail; orchestration is the product. | Validated v0.1.0 |
| Git as coordination substrate | No external dependencies. Inspectable with standard tools. Offline-capable. | Validated v0.1.0 |
| Human interaction via forge, not custom app | Developers already live in GitHub/ADO/GitLab. Building a custom UI is massive surface area with no clear advantage. | Pending (forge integration deferred) |
| Assay boundary: session-level quality, not cross-session orchestration | Assay owns specs + gates within a session. Smelt owns everything between sessions. | Pending (Assay integration deferred) |
| Language: Rust | Ecosystem alignment with Assay, single-binary distribution, tokio async, strong typing for complex state machines. | Validated v0.1.0 |
| Shell-out to git CLI behind trait | Avoids git2/gix maturity gaps for write operations. Trait abstraction allows future swap. | Validated v0.1.0 |
| Sequential merge (not octopus) | Isolates conflicts to specific branch pairs. Simpler to reason about and resolve. | Validated v0.1.0 |
| Human fallback before AI resolution | Safety net first, optimization second. Ensures working fallback path always exists. | Validated v0.1.0 |
| RPITIT for async traits | No async-trait crate needed. Native Rust 1.85+ feature. | Validated v0.1.0 |

---
*Last updated: 2026-03-11 — v0.1.0 milestone complete*
