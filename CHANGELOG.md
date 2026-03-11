# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-03-11

### Added

- Multi-agent session orchestration with parallel execution via DAG-based task graph
- Git worktree lifecycle management (create, list, remove, prune, orphan detection)
- Session manifest format (TOML) for defining agent tasks and dependencies
- Scripted/simulated sessions for development and testing with configurable failure modes
- Sequential squash merge of agent worktree outputs into a single target branch
- Merge order intelligence with file-overlap-based and completion-time strategies
- AI-assisted conflict resolution with LLM provider abstraction (Anthropic, OpenAI, Ollama, Gemini)
- Human fallback resolution with interactive 3-option menu (Resolve/Skip/Abort)
- Real Claude Code agent session backend with CLAUDE.md and settings injection
- Per-session contribution summary with files changed, lines added/removed, commit messages
- Scope isolation verification — flags sessions modifying files outside assigned scope
- Git-native orchestration state (no external database or message queue)
- Orchestration plan with dependency edges, failure policies, and crash recovery via resume
- Live progress dashboard with indicatif spinners and comfy-table summaries
- JSON output mode for all commands
- Graceful Ctrl-C shutdown with process group management
