---
title: "Improve error handling in CLI session command"
area: "smelt-cli"
priority: low
source: "PR #13 review"
---

In `session.rs:28-34` (CLI), error handling uses `eprintln!` + `return Ok(1)` which loses the error chain. Consider using `anyhow::bail!` or returning `Err` to preserve error context for callers.
