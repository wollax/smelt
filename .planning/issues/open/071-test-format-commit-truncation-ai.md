# format_commit_message tests don't cover truncation with AI suffix

**Source:** PR #17 review round 2 (suggestion)
**Area:** testing
**Phase:** 7

## Description

AI resolution suffixes (`[resolved: ai-assisted]`) consume extra characters from the 72-char budget. No test exercises a long task description with an AI resolution method to verify correct truncation behavior.
