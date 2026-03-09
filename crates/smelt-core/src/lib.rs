//! Smelt core library — git operations, orchestration logic, and domain types.

pub mod error;
pub mod git;

pub use error::{Result, SmeltError};
pub use git::{preflight, GitCli, GitOps};
