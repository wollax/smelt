# strip_code_fences missing nested fence test

**Source:** PR #17 review round 2 (suggestion)
**Area:** testing
**Phase:** 7

## Description

If the LLM resolves a Markdown file containing code fences, `strip_code_fences` may incorrectly strip the outer fence and expose inner fences. Add a test with nested triple-backtick content.
