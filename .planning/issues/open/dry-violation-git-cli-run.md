---
title: "DRY violation between run and run_in in git CLI"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `git/cli.rs:30-53` and `56-79`, there is a DRY violation between `run` and `run_in`. The `run` method could delegate to `run_in(&self.repo_root, args)` to eliminate the duplication.
