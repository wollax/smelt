---
id: "049"
area: smelt-core
severity: suggestion
source: pr-review-phase-06
---
# ConflictHandler RPITIT prevents dyn dispatch

Using RPITIT (return-position impl Trait in trait) for async handlers prevents runtime-selected handler dispatch via `dyn ConflictHandler`. Consider returning `BoxFuture` to allow `dyn`-compatible usage in the future.

File: `merge/mod.rs`
