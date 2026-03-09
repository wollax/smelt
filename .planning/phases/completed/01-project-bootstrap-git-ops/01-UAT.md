---
phase: 01-project-bootstrap-git-ops
status: passed
started: 2026-03-09
completed: 2026-03-09
tests_total: 7
tests_passed: 7
tests_failed: 0
---

# Phase 1 UAT: Project Bootstrap & Git Operations Layer

## Tests

| # | Test | Expected | Status |
|---|------|----------|--------|
| 1 | `smelt --version` | Prints "smelt 0.1.0" and exits 0 | PASS |
| 2 | `smelt --help` | Shows available commands including `init` | PASS |
| 3 | `smelt init` in a git repo | Creates `.smelt/config.toml`, prints success message | PASS |
| 4 | `smelt init` twice | Second run prints "already" error, exits non-zero | PASS |
| 5 | `smelt` (no args, no .smelt/) | Prints "Not a Smelt project" error | PASS |
| 6 | `smelt` (no args, after init) | Shows project status with branch name | PASS |
| 7 | `smelt` outside a git repo | Prints clear git-related error, exits non-zero | PASS |
