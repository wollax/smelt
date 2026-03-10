//! Types for merge operations and reporting.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Strategy for ordering sessions during merge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "kebab-case")]
pub enum MergeOrderStrategy {
    /// Order sessions by manifest position (default — preserves Phase 4 behavior).
    #[default]
    CompletionTime,
    /// Order sessions by file overlap — merge least-overlapping first.
    FileOverlap,
}

impl fmt::Display for MergeOrderStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CompletionTime => write!(f, "completion-time"),
            Self::FileOverlap => write!(f, "file-overlap"),
        }
    }
}

impl FromStr for MergeOrderStrategy {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "completion-time" => Ok(Self::CompletionTime),
            "file-overlap" => Ok(Self::FileOverlap),
            _ => Err(format!(
                "unknown strategy '{s}' (expected: completion-time, file-overlap)"
            )),
        }
    }
}

/// Options for a merge operation.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct MergeOpts {
    /// Override the target branch name (default: `smelt/merge/<manifest-name>`).
    pub target_branch: Option<String>,
    /// Override the merge ordering strategy.
    pub strategy: Option<MergeOrderStrategy>,
}

impl MergeOpts {
    /// Create merge options with both target branch and strategy overrides.
    pub fn new(
        target_branch: Option<String>,
        strategy: Option<MergeOrderStrategy>,
    ) -> Self {
        Self {
            target_branch,
            strategy,
        }
    }

    /// Create merge options with a custom target branch.
    pub fn with_target_branch(target: String) -> Self {
        Self {
            target_branch: Some(target),
            strategy: None,
        }
    }

    /// Create merge options with a specific ordering strategy.
    pub fn with_strategy(strategy: MergeOrderStrategy) -> Self {
        Self {
            target_branch: None,
            strategy: Some(strategy),
        }
    }
}

/// Per-file diff statistics.
#[derive(Debug, Clone, Serialize)]
pub struct DiffStat {
    pub file: String,
    pub insertions: usize,
    pub deletions: usize,
}

/// Result of merging a single session.
#[derive(Debug, Clone, Serialize)]
pub struct MergeSessionResult {
    pub session_name: String,
    pub commit_hash: String,
    pub diff_stats: Vec<DiffStat>,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

/// Overall merge report.
#[derive(Debug, Clone, Serialize)]
pub struct MergeReport {
    pub target_branch: String,
    pub base_commit: String,
    pub sessions_merged: Vec<MergeSessionResult>,
    pub sessions_skipped: Vec<String>,
    pub total_files_changed: usize,
    pub total_insertions: usize,
    pub total_deletions: usize,
    /// The merge plan describing session ordering and overlap analysis.
    pub plan: Option<MergePlan>,
}

impl MergeReport {
    /// Returns `true` if any sessions were skipped during the merge.
    pub fn has_skipped(&self) -> bool {
        !self.sessions_skipped.is_empty()
    }
}

/// A merge plan showing the computed session order and overlap analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergePlan {
    /// The strategy used for ordering.
    pub strategy: MergeOrderStrategy,
    /// Whether the strategy fell back to completion-time due to no meaningful differentiation.
    pub fell_back: bool,
    /// Ordered list of sessions to merge.
    pub sessions: Vec<SessionPlanEntry>,
    /// Pairwise overlap scores (only populated for FileOverlap strategy).
    pub pairwise_overlaps: Vec<PairwiseOverlap>,
}

/// A session entry in the merge plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPlanEntry {
    pub session_name: String,
    pub branch_name: String,
    pub changed_files: Vec<String>,
    /// Position in the original manifest order (0-indexed).
    pub original_index: usize,
}

impl SessionPlanEntry {
    /// Number of changed files.
    pub fn file_count(&self) -> usize {
        self.changed_files.len()
    }
}

/// Pairwise file overlap between two sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairwiseOverlap {
    pub session_a: String,
    pub session_b: String,
    pub overlapping_files: Vec<String>,
}

impl PairwiseOverlap {
    /// Number of overlapping files.
    pub fn overlap_count(&self) -> usize {
        self.overlapping_files.len()
    }
}
