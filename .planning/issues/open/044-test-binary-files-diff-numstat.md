---
id: "044"
area: smelt-core
severity: suggestion
source: pr-review-phase-04
---
# diff_numstat with binary files untested

Binary files produce `-` instead of numbers in numstat output. Code uses `unwrap_or(0)` which handles it, but no test confirms the behavior.
