---
title: "Assert skipped steps don't execute in exit_after test"
area: "smelt-cli"
priority: low
source: "PR #13 review"
---

In `cli_session.rs:174-179`, the `exit_after` test doesn't verify that the second step was NOT executed. Assert that `b.txt`/`c.txt` don't exist to confirm subsequent steps were skipped.
