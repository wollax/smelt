# Retry prompt still has empty base/theirs context

**Source:** PR #17 review round 2 (suggestion)
**Area:** smelt-cli/merge
**Phase:** 7

## Description

`retry_with_feedback` now correctly uses original conflict content as "ours", but base and theirs are still passed as empty strings since the 3-way context from git index stages is not preserved across retries. Consider caching stage content alongside original_contents for richer retry prompts.
