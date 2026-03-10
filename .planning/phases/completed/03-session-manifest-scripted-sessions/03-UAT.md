# Phase 03 UAT: Session Manifest & Scripted Sessions

**Phase:** 03 — Session Manifest & Scripted Sessions
**Date:** 2026-03-09
**Status:** Complete

## Tests

| # | Test | Status |
|---|------|--------|
| 1 | `smelt session --help` shows run subcommand | PASS |
| 2 | 2-session manifest creates worktrees and commits | PASS |
| 3 | Two sessions editing same file produce different content | PASS |
| 4 | `exit_after` truncates execution to N steps | PASS |
| 5 | `simulate_failure = "crash"` returns non-zero exit | PASS |
| 6 | Invalid manifest path returns clear error | PASS |
| 7 | Running without `smelt init` returns clear error | PASS |

## Results

7/7 tests passed. All Phase 03 deliverables verified manually.
