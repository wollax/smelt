# Duplicate progress messages in merge output

**Source:** PR #17 review (suggestion)
**Area:** smelt-cli/merge
**Phase:** 4

## Description

`execute_merge_run` prints per-session progress (`[1/N] Merged 'sess'`) and then a summary (`Merged N session(s) into 'branch'`). The per-session lines duplicate information available in the summary. Consider consolidating to reduce output noise, or making per-session progress only show in verbose mode.
