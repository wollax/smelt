# AiPromptChoice index-coupled to dialoguer item order

**Source:** PR #17 review round 2 (suggestion)
**Area:** smelt-cli/merge
**Phase:** 7

## Description

`prompt_accept_edit_reject` returns a raw `usize` index, and `show_diff_and_prompt` maps it to `AiPromptChoice` via magic numbers. If the dialoguer item list order changes, the mapping silently breaks. Move the variant construction inside the spawned closure to eliminate the integer-to-variant mapping.
