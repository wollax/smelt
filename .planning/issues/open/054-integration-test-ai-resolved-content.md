# Add integration test verifying AI-resolved merged content

**Source:** PR #17 review (suggestion)
**Area:** testing
**Phase:** 7

## Description

Add an integration test that verifies the end-to-end flow of AI conflict resolution: create conflicting sessions, run merge with a mock AI provider, and assert the final merged content matches expectations. Current tests only verify CLI exit codes and flag behavior.
