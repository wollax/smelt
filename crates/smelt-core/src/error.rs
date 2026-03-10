//! Unified error type for Smelt core operations.

use std::path::PathBuf;

use thiserror::Error;

/// Unified error type for Smelt core operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SmeltError {
    /// `git` binary not found on `$PATH`.
    #[error("`git` not found on $PATH. Smelt requires git to be installed.")]
    GitNotFound,

    /// Current directory is not inside a git repository.
    #[error("not a git repository (or any parent up to mount point)")]
    NotAGitRepo,

    /// A git command failed.
    #[error("git {operation} failed: {message}")]
    GitExecution { operation: String, message: String },

    /// `.smelt/` already exists in the repository.
    #[error(".smelt/ already exists in {path}. Already initialized.")]
    AlreadyInitialized { path: PathBuf },

    /// An I/O operation failed with context.
    #[error("{operation} at `{path}`: {source}")]
    Io {
        operation: String,
        path: PathBuf,
        source: std::io::Error,
    },

    /// Worktree with this name already exists.
    #[error("worktree '{name}' already exists")]
    WorktreeExists { name: String },

    /// Worktree not found in Smelt state.
    #[error("worktree '{name}' not found")]
    WorktreeNotFound { name: String },

    /// Branch already exists (collision).
    #[error("branch '{branch}' already exists")]
    BranchExists { branch: String },

    /// Worktree has uncommitted changes.
    #[error("worktree '{name}' has uncommitted changes (use --force to override)")]
    WorktreeDirty { name: String },

    /// Branch has unmerged commits.
    #[error("branch '{branch}' has unmerged commits (use --force to delete)")]
    BranchUnmerged { branch: String },

    /// Smelt project not initialized.
    #[error("not a Smelt project (run `smelt init` first)")]
    NotInitialized,

    /// State file deserialization error.
    #[error("failed to parse state file: {0}")]
    StateDeserialization(String),

    /// Manifest parsing or validation error.
    #[error("manifest error: {0}")]
    ManifestParse(String),

    /// Session-specific error.
    #[error("session '{session}': {message}")]
    SessionError { session: String, message: String },
}

impl SmeltError {
    /// Convenience constructor for the [`Io`](SmeltError::Io) variant.
    pub fn io(
        operation: impl Into<String>,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        Self::Io {
            operation: operation.into(),
            path: path.into(),
            source,
        }
    }
}

/// A `Result` alias that uses [`SmeltError`] as the error type.
pub type Result<T> = std::result::Result<T, SmeltError>;
