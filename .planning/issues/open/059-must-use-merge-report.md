# Add #[must_use] to MergeReport

**Source:** PR #17 review (suggestion)
**Area:** smelt-core/merge
**Phase:** 4

## Description

`MergeReport` is the return type of `MergeRunner::run()` and contains important merge results. Add `#[must_use]` to ensure callers don't accidentally discard it.
