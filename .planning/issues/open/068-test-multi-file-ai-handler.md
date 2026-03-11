# Add multi-file test for AiConflictHandler

**Source:** PR #17 review round 2 (suggestion)
**Area:** testing
**Phase:** 7

## Description

`handle_conflict` loops over files but the only happy-path test uses a single file. Add a two-file test verifying: (a) provider called N times, (b) each file written to correct path, (c) partial failure doesn't swallow other files.
