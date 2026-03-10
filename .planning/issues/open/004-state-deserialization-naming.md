---
id: "004"
area: smelt-core
severity: important
source: pr-review-phase-02
---

# StateDeserialization variant name is misleading

`SmeltError::StateDeserialization` is used for both serialization and deserialization errors. Consider renaming to `StateSerialization` or splitting into two variants for clarity.

**File:** `crates/smelt-core/src/error.rs`
