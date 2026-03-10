---
title: "Verify let-chain syntax MSRV compatibility"
area: "smelt-core"
priority: low
source: "PR #13 review"
---

In `manifest.rs:145-152`, let-chain syntax is used which requires Rust 1.87.0+, but `rust-version` in Cargo.toml is set to 1.85. Verify MSRV compatibility and either bump `rust-version` or rewrite to avoid let-chains.
