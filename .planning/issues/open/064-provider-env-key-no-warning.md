# No warning when provider_to_env_key returns None for configured provider

**Source:** PR #17 review round 2 (suggestion)
**Area:** smelt-core/ai
**Phase:** 7

## Description

When `provider_to_env_key` returns `None` for a user-configured provider (e.g., "custom"), the API key from config is silently ignored. Add a `tracing::warn!` at the `None` branch so users can diagnose authentication failures.
