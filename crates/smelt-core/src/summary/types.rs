//! Summary report types for session analysis and scope violation tracking.

use serde::{Deserialize, Serialize};

/// Per-file diff statistics.
///
/// Binary files have `insertions = 0` and `deletions = 0` (from `diff --numstat`
/// `-\t-\tfile` output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStat {
    pub path: String,
    pub insertions: usize,
    pub deletions: usize,
}

/// A file that was changed outside a session's declared `file_scope`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeViolation {
    /// Name of the session that touched the out-of-scope file.
    pub session_name: String,
    /// Path of the file that is out of scope.
    pub file_path: String,
    /// The `file_scope` globs that were active for this session (diagnostic context).
    pub file_scope: Vec<String>,
}

/// Per-session summary of changes, commit messages, and scope violations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_name: String,
    pub files: Vec<FileStat>,
    pub total_insertions: usize,
    pub total_deletions: usize,
    pub commit_messages: Vec<String>,
    pub violations: Vec<ScopeViolation>,
}

impl SessionSummary {
    /// Number of files changed in this session.
    pub fn files_changed(&self) -> usize {
        self.files.len()
    }

    /// Returns `true` if this session has any scope violations.
    pub fn has_violations(&self) -> bool {
        !self.violations.is_empty()
    }
}

/// Aggregate totals across all sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryTotals {
    pub sessions: usize,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub violations: usize,
}

/// Top-level summary report for an entire orchestration run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryReport {
    pub manifest_name: String,
    pub run_id: String,
    pub base_ref: String,
    pub sessions: Vec<SessionSummary>,
    pub totals: SummaryTotals,
}

impl SummaryReport {
    /// Returns `true` if any session has scope violations.
    pub fn has_violations(&self) -> bool {
        self.totals.violations > 0
    }

    /// Collect all scope violations across all sessions.
    pub fn all_violations(&self) -> Vec<&ScopeViolation> {
        self.sessions
            .iter()
            .flat_map(|s| &s.violations)
            .collect()
    }
}
