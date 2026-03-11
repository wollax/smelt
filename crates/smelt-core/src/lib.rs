//! Smelt core library — git operations, orchestration logic, and domain types.

pub mod ai;
pub mod error;
pub mod git;
pub mod init;
pub mod merge;
pub mod orchestrate;
pub mod session;
pub mod summary;
pub mod worktree;

pub use ai::{AiConfig, AiProvider, GenAiProvider};
pub use error::{Result, SmeltError};
pub use git::{GitCli, GitOps, preflight};
pub use init::init_project;
pub use merge::{
    AiConflictHandler, ConflictAction, ConflictHandler, MergeOpts, MergeOrderStrategy, MergePlan,
    MergeReport, NoopConflictHandler, ResolutionMethod,
};
pub use orchestrate::{
    build_dag, FailurePolicy, Orchestrator, OrchestrationOpts, OrchestrationReport, RunState,
    RunStateManager, SessionDag,
};
pub use session::{Manifest, SessionResult, SessionRunner};
pub use summary::{
    collect_summary, FileStat, ScopeViolation, SessionSummary, SummaryReport, SummaryTotals,
};
pub use worktree::{CreateWorktreeOpts, RemoveResult, WorktreeInfo, WorktreeManager};
