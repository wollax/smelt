//! Smelt core library — git operations, orchestration logic, and domain types.

pub mod error;
pub mod git;
pub mod init;
pub mod merge;
pub mod session;
pub mod worktree;

pub use error::{Result, SmeltError};
pub use git::{GitCli, GitOps, preflight};
pub use init::init_project;
pub use merge::{MergeOpts, MergeReport};
pub use session::{Manifest, SessionResult, SessionRunner};
pub use worktree::{CreateWorktreeOpts, RemoveResult, WorktreeInfo, WorktreeManager};
