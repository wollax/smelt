# lib.rs has incomplete re-exports

**Source:** PR #17 review (suggestion)
**Area:** smelt-core
**Phase:** 7

## Description

`smelt-core/src/lib.rs` re-exports some types but not all public API types (e.g., `AiConfig`, `AiProvider`, `GenAiProvider` are accessed via `smelt_core::ai::*` rather than the root). Audit and decide on a consistent re-export strategy — either re-export all public types from lib.rs or document that sub-modules are the canonical paths.
