# Smelt

Multi-agent orchestration for autonomous software development. Smelt coordinates AI coding agent sessions in git worktrees and merges their outputs into a single coherent branch.

## What It Does

Smelt handles what happens *between* agent sessions:

- **Worktree management** — create, track, and clean up isolated git worktrees for each agent
- **Session orchestration** — run multiple agents in parallel via a DAG-based task graph
- **Merge pipeline** — sequential squash merge with intelligent file-overlap-based ordering
- **Conflict resolution** — AI-assisted resolution with human fallback
- **Scope verification** — flag agents that modified files outside their assigned scope
- **Session summaries** — per-agent contribution reports (files, lines, commits)

## Quick Start

```bash
# Build from source
cargo install --path crates/smelt-cli

# Initialize in a git repo
smelt init

# Run an orchestration from a manifest
smelt orchestrate run manifest.toml
```

### Example Manifest

```toml
[manifest]
name = "feature-work"
target_branch = "main"
merge_strategy = "file-overlap"

[[session]]
name = "api-changes"
task = "Add REST endpoints for user management"
file_scope = ["src/api/**", "src/models/user.rs"]

[[session]]
name = "ui-updates"
task = "Build user management UI components"
file_scope = ["src/components/**", "src/pages/users/**"]
```

## Commands

| Command | Description |
|---------|-------------|
| `smelt init` | Initialize `.smelt/` in the current repo |
| `smelt worktree create\|list\|remove\|prune` | Manage agent worktrees |
| `smelt session run <manifest>` | Run sessions from a manifest |
| `smelt merge run\|plan <manifest>` | Merge session outputs (or preview the plan) |
| `smelt orchestrate run <manifest>` | Full lifecycle: worktrees → sessions → merge → summary |
| `smelt summary [--run-id <id>]` | View per-session contribution summaries |

### Aliases

- `smelt wt` → `smelt worktree`
- `smelt orch` → `smelt orchestrate`

### Common Flags

- `--target <branch>` — merge target branch (default: smelt/merged)
- `--strategy <completion-time|file-overlap>` — merge ordering strategy
- `--no-ai` — disable AI conflict resolution
- `--verbose` — show detailed conflict context
- `--json` — JSON output to stdout
- `--no-color` — disable terminal colors

## Agent Backends

- **Scripted sessions** — deterministic scripts for testing (commits, file edits, configurable failures)
- **Claude Code** — real AI agent sessions with task prompts from the manifest

## AI Conflict Resolution

When merge conflicts occur, Smelt:

1. Extracts 3-way merge context (base, ours, theirs) from git index stages
2. Sends context + session descriptions to an LLM
3. Presents the proposed resolution as a colored diff
4. Offers Accept / Edit / Reject with retry-with-feedback
5. Falls back to interactive manual resolution if AI is rejected

Supported providers: Anthropic, OpenAI, Ollama, Gemini (via [genai](https://crates.io/crates/genai)).

Configure in `.smelt/config.toml`:

```toml
[ai]
enabled = true
provider = "anthropic"
model = "claude-sonnet-4"
```

## Requirements

- Rust 1.85+ (Edition 2024)
- Git 2.20+
- Claude Code CLI (for real agent sessions)

## License

MIT
