//! Session result and outcome types.

use std::time::Duration;

/// Outcome of a session execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionOutcome {
    Completed,
    Failed,
    TimedOut,
    Killed,
}

/// Result of a completed (or failed) session execution.
#[derive(Debug, Clone)]
pub struct SessionResult {
    pub session_name: String,
    pub outcome: SessionOutcome,
    pub steps_completed: usize,
    pub failure_reason: Option<String>,
    pub has_commits: bool,
    pub duration: Duration,
}
