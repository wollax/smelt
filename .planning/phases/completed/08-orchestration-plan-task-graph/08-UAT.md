# Phase 08 UAT: Orchestration Plan & Task Graph

**Phase:** 08 — Orchestration Plan & Task Graph
**Date:** 2026-03-10
**Status:** PASSED (10/10)

## Tests

| # | Test | Status | Notes |
|---|------|--------|-------|
| 1 | `smelt orchestrate --help` shows command with flags | PASS | All flags present: --target, --strategy, --verbose, --no-ai, --json |
| 2 | `smelt orch --help` alias works | PASS | Identical output to `orchestrate` |
| 3 | Two parallel sessions merge into target branch | PASS | Both files in merged branch, sessions ran concurrently |
| 4 | Sequential dependency (A→B) respects order | PASS | B waited for A to complete |
| 5 | Diamond dependency (A→{B,C}→D) all complete | PASS | base→left+right parallel→top sequential, 4 files merged |
| 6 | Failed session with skip-dependents skips deps, runs independents | PASS | Dependent skipped with reason, independent completed, exit code 1 |
| 7 | Failed session with abort policy stops everything | PASS | Immediate abort, remaining cancelled, exit code 1 |
| 8 | `--json` outputs valid OrchestrationReport JSON | PASS | Full report with run_id, session_results, merge_report, outcome |
| 9 | `parallel_by_default = false` runs sessions sequentially | PASS | Sessions ran one-at-a-time in manifest order |
| 10 | Manifest with cycle in depends_on rejected | PASS | Clear error at parse time listing cycle participants |
