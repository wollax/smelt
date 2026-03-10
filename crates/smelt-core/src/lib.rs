//! Smelt core library — git operations, orchestration logic, and domain types.

pub mod error;
pub mod git;
pub mod init;
pub mod session;
pub mod worktree;

pub use error::{Result, SmeltError};
pub use git::{GitCli, GitOps, preflight};
pub use init::init_project;
pub use session::{Manifest, SessionResult};
pub use worktree::{CreateWorktreeOpts, RemoveResult, WorktreeInfo, WorktreeManager};
