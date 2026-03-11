# ResolutionMethod missing Deserialize for round-trip

**Source:** PR #17 review round 2 (suggestion)
**Area:** smelt-core/merge
**Phase:** 7

## Description

`ResolutionMethod` derives `Serialize` but not `Deserialize`. Adding `Deserialize` while variants are stable enables round-tripping `MergeSessionResult` from JSON.
