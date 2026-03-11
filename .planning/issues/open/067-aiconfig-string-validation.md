# AiConfig string fields have no validation

**Source:** PR #17 review round 2 (suggestion)
**Area:** smelt-core/ai
**Phase:** 7

## Description

`provider`, `model`, `api_key`, and `endpoint` are all raw `Option<String>` with no validation. Empty strings or invalid URLs can reach `GenAiProvider`. Consider a `validate()` method or newtype wrappers.
