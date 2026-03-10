//! Types for merge operations and reporting.

/// Options for a merge operation.
#[derive(Debug, Clone)]
pub struct MergeOpts {
    /// Override the target branch name (default: `smelt/merge/<manifest-name>`).
    pub target_branch: Option<String>,
}

impl Default for MergeOpts {
    fn default() -> Self {
        Self {
            target_branch: None,
        }
    }
}

/// Per-file diff statistics.
#[derive(Debug, Clone)]
pub struct DiffStat {
    pub file: String,
    pub insertions: usize,
    pub deletions: usize,
}

/// Result of merging a single session.
#[derive(Debug, Clone)]
pub struct MergeSessionResult {
    pub session_name: String,
    pub commit_hash: String,
    pub diff_stats: Vec<DiffStat>,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

/// Overall merge report.
#[derive(Debug, Clone)]
pub struct MergeReport {
    pub target_branch: String,
    pub base_commit: String,
    pub sessions_merged: Vec<MergeSessionResult>,
    pub sessions_skipped: Vec<String>,
    pub total_files_changed: usize,
    pub total_insertions: usize,
    pub total_deletions: usize,
}
