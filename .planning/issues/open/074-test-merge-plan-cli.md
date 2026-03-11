# No CLI integration test for merge plan subcommand

**Source:** PR #17 review round 2 (suggestion)
**Area:** testing
**Phase:** 5

## Description

`smelt merge plan` is only tested via unit tests on `format_plan_table` and JSON round-trip. No integration test runs the CLI end-to-end with `--json` flag.
