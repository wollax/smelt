# Integration tests don't verify AI bypass or config-disabled log messages

**Source:** PR #17 review round 2 (suggestion)
**Area:** testing
**Phase:** 7

## Description

`test_merge_no_ai_flag_clean_merge` doesn't verify AI was bypassed. `test_merge_ai_disabled_config_conflict_exits_error` doesn't check for the "AI resolution disabled" stderr message.
