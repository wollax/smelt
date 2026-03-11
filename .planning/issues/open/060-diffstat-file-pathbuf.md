# DiffStat.file should be PathBuf not String

**Source:** PR #17 review (suggestion)
**Area:** smelt-core/merge
**Phase:** 4

## Description

`DiffStat` uses `String` for the `file` field. Since it represents a file path, using `PathBuf` would be more type-safe and consistent with other path-carrying types in the codebase.
