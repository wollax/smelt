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
