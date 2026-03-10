//! Orchestration state, policy, and reporting types.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::merge::types::{MergeOrderStrategy, MergeReport};

/// Failure policy for orchestration — governs behavior when a session fails.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailurePolicy {
    /// Dependents of a failed session are skipped; independent sessions continue.
    #[default]
    SkipDependents,
    /// Any session failure stops the entire orchestration.
    Abort,
}

/// State of a single session within an orchestration run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum SessionRunState {
    /// Waiting for dependencies to complete.
    Pending,
    /// Currently executing.
    Running,
    /// Finished successfully.
    Completed {
        /// Wall-clock duration in seconds.
        duration_secs: f64,
    },
    /// Finished with an error.
    Failed {
        /// Human-readable reason for failure.
        reason: String,
    },
    /// Skipped because a dependency failed.
    Skipped {
        /// Human-readable reason for skipping.
        reason: String,
    },
    /// Cancelled by the user or by abort policy.
    Cancelled,
}

impl SessionRunState {
    /// Returns `true` if this state is terminal (no further transitions).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed { .. } | Self::Failed { .. } | Self::Skipped { .. } | Self::Cancelled
        )
    }

    /// Returns `true` if the session completed successfully.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Completed { .. })
    }
}

/// High-level phase of an orchestration run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunPhase {
    /// Executing sessions.
    Sessions,
    /// Running merge pipeline.
    Merging,
    /// All done successfully.
    Complete,
    /// Orchestration failed.
    Failed,
}

/// Progress of the merge phase within an orchestration run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeProgress {
    /// Sessions successfully merged so far.
    pub sessions_merged: Vec<String>,
    /// Session currently being merged (if any).
    pub current_session: Option<String>,
}

/// Persistent orchestration state for crash recovery and resume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    /// Unique identifier for this run.
    pub run_id: String,
    /// Name of the manifest being executed.
    pub manifest_name: String,
    /// SHA-256 hex digest of the manifest content (for resume validation).
    pub manifest_hash: String,
    /// When the run started.
    pub started_at: DateTime<Utc>,
    /// When the state was last persisted.
    pub updated_at: DateTime<Utc>,
    /// Current lifecycle phase.
    pub phase: RunPhase,
    /// Per-session execution state.
    pub sessions: HashMap<String, SessionRunState>,
    /// Progress through the merge phase (if applicable).
    pub merge_progress: Option<MergeProgress>,
    /// Failure policy in effect.
    pub failure_policy: FailurePolicy,
}

impl RunState {
    /// Create a new run state with all sessions in `Pending`.
    pub fn new(
        run_id: String,
        manifest_name: String,
        manifest_hash: String,
        failure_policy: FailurePolicy,
        session_names: &[String],
    ) -> Self {
        let now = Utc::now();
        let sessions = session_names
            .iter()
            .map(|name| (name.clone(), SessionRunState::Pending))
            .collect();

        Self {
            run_id,
            manifest_name,
            manifest_hash,
            started_at: now,
            updated_at: now,
            phase: RunPhase::Sessions,
            sessions,
            merge_progress: None,
            failure_policy,
        }
    }

    /// Persist the run state to `state.json` in the given directory.
    pub fn save(&self, dir: &Path) -> crate::Result<()> {
        std::fs::create_dir_all(dir)
            .map_err(|e| crate::SmeltError::io("creating run directory", dir, e))?;

        let path = dir.join("state.json");
        let json = serde_json::to_string_pretty(self).map_err(|e| {
            crate::SmeltError::Orchestration {
                message: format!("failed to serialize run state: {e}"),
            }
        })?;

        std::fs::write(&path, json)
            .map_err(|e| crate::SmeltError::io("writing run state", &path, e))?;

        Ok(())
    }

    /// Load a run state from `state.json` in the given directory.
    pub fn load(dir: &Path) -> crate::Result<Self> {
        let path = dir.join("state.json");
        let json = std::fs::read_to_string(&path)
            .map_err(|e| crate::SmeltError::io("reading run state", &path, e))?;

        serde_json::from_str(&json).map_err(|e| crate::SmeltError::Orchestration {
            message: format!("failed to deserialize run state: {e}"),
        })
    }

    /// Returns `true` if the run completed successfully.
    pub fn is_complete(&self) -> bool {
        self.phase == RunPhase::Complete
    }

    /// Returns `true` if the run can be resumed (sessions or merging phase).
    pub fn is_resumable(&self) -> bool {
        matches!(self.phase, RunPhase::Sessions | RunPhase::Merging)
    }

    /// Generate a run ID from the manifest name and current UTC timestamp.
    ///
    /// Format: `<manifest_name>-<YYYYMMDD-HHMMSS>`
    pub fn generate_run_id(manifest_name: &str) -> String {
        let now = Utc::now();
        format!("{}-{}", manifest_name, now.format("%Y%m%d-%H%M%S"))
    }
}

/// Options for an orchestration run.
#[derive(Debug, Clone, Default)]
pub struct OrchestrationOpts {
    /// Override the target branch for the merge phase.
    pub target_branch: Option<String>,
    /// Override the merge ordering strategy.
    pub strategy: Option<MergeOrderStrategy>,
    /// Enable verbose output (stream session stdout/stderr).
    pub verbose: bool,
    /// Disable AI conflict resolution.
    pub no_ai: bool,
    /// Output structured JSON instead of human-readable output.
    pub json: bool,
}

/// Final report of an orchestration run.
#[derive(Debug, Clone, Serialize)]
pub struct OrchestrationReport {
    /// Unique identifier for this run.
    pub run_id: String,
    /// Name of the manifest that was executed.
    pub manifest_name: String,
    /// Per-session results.
    pub session_results: HashMap<String, SessionRunState>,
    /// Merge report (if merge phase was reached).
    pub merge_report: Option<MergeReport>,
    /// Total wall-clock elapsed time in seconds.
    pub elapsed_secs: f64,
    /// Final outcome.
    pub outcome: RunPhase,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_policy_default_is_skip_dependents() {
        assert_eq!(FailurePolicy::default(), FailurePolicy::SkipDependents);
    }

    #[test]
    fn session_run_state_pending_not_terminal() {
        assert!(!SessionRunState::Pending.is_terminal());
        assert!(!SessionRunState::Pending.is_success());
    }

    #[test]
    fn session_run_state_running_not_terminal() {
        assert!(!SessionRunState::Running.is_terminal());
        assert!(!SessionRunState::Running.is_success());
    }

    #[test]
    fn session_run_state_completed_is_terminal_and_success() {
        let state = SessionRunState::Completed {
            duration_secs: 5.0,
        };
        assert!(state.is_terminal());
        assert!(state.is_success());
    }

    #[test]
    fn session_run_state_failed_is_terminal_not_success() {
        let state = SessionRunState::Failed {
            reason: "boom".to_string(),
        };
        assert!(state.is_terminal());
        assert!(!state.is_success());
    }

    #[test]
    fn session_run_state_skipped_is_terminal_not_success() {
        let state = SessionRunState::Skipped {
            reason: "dep failed".to_string(),
        };
        assert!(state.is_terminal());
        assert!(!state.is_success());
    }

    #[test]
    fn session_run_state_cancelled_is_terminal_not_success() {
        assert!(SessionRunState::Cancelled.is_terminal());
        assert!(!SessionRunState::Cancelled.is_success());
    }

    #[test]
    fn run_state_new_initializes_all_pending() {
        let state = RunState::new(
            "test-run".to_string(),
            "manifest".to_string(),
            "abc123".to_string(),
            FailurePolicy::SkipDependents,
            &["a".to_string(), "b".to_string(), "c".to_string()],
        );

        assert_eq!(state.run_id, "test-run");
        assert_eq!(state.sessions.len(), 3);
        assert!(state.sessions.values().all(|s| matches!(s, SessionRunState::Pending)));
        assert_eq!(state.phase, RunPhase::Sessions);
        assert!(!state.is_complete());
        assert!(state.is_resumable());
    }

    #[test]
    fn run_state_save_and_load_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = RunState::new(
            "round-trip".to_string(),
            "test".to_string(),
            "hash123".to_string(),
            FailurePolicy::Abort,
            &["s1".to_string()],
        );

        state.save(dir.path()).expect("save");
        let loaded = RunState::load(dir.path()).expect("load");

        assert_eq!(loaded.run_id, "round-trip");
        assert_eq!(loaded.manifest_name, "test");
        assert_eq!(loaded.manifest_hash, "hash123");
        assert_eq!(loaded.failure_policy, FailurePolicy::Abort);
        assert_eq!(loaded.sessions.len(), 1);
    }

    #[test]
    fn run_state_generate_run_id_format() {
        let id = RunState::generate_run_id("my-feature");
        assert!(id.starts_with("my-feature-"));
        // Should have the format: my-feature-YYYYMMDD-HHMMSS
        assert!(id.len() > "my-feature-".len() + 10);
    }

    #[test]
    fn run_state_phase_complete() {
        let mut state = RunState::new(
            "test".to_string(),
            "m".to_string(),
            "h".to_string(),
            FailurePolicy::default(),
            &[],
        );
        state.phase = RunPhase::Complete;
        assert!(state.is_complete());
        assert!(!state.is_resumable());
    }

    #[test]
    fn run_state_phase_failed_not_resumable() {
        let mut state = RunState::new(
            "test".to_string(),
            "m".to_string(),
            "h".to_string(),
            FailurePolicy::default(),
            &[],
        );
        state.phase = RunPhase::Failed;
        assert!(!state.is_complete());
        assert!(!state.is_resumable());
    }

    #[test]
    fn run_state_merging_is_resumable() {
        let mut state = RunState::new(
            "test".to_string(),
            "m".to_string(),
            "h".to_string(),
            FailurePolicy::default(),
            &[],
        );
        state.phase = RunPhase::Merging;
        assert!(!state.is_complete());
        assert!(state.is_resumable());
    }

    #[test]
    fn failure_policy_serde_round_trip() {
        let json = serde_json::to_string(&FailurePolicy::Abort).expect("serialize");
        assert_eq!(json, "\"abort\"");
        let back: FailurePolicy = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, FailurePolicy::Abort);

        let json = serde_json::to_string(&FailurePolicy::SkipDependents).expect("serialize");
        assert_eq!(json, "\"skip-dependents\"");
        let back: FailurePolicy = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, FailurePolicy::SkipDependents);
    }

    #[test]
    fn session_run_state_serde_round_trip() {
        let state = SessionRunState::Completed {
            duration_secs: 1.5,
        };
        let json = serde_json::to_string(&state).expect("serialize");
        let back: SessionRunState = serde_json::from_str(&json).expect("deserialize");
        assert!(back.is_success());

        let state = SessionRunState::Failed {
            reason: "timeout".to_string(),
        };
        let json = serde_json::to_string(&state).expect("serialize");
        let back: SessionRunState = serde_json::from_str(&json).expect("deserialize");
        assert!(back.is_terminal());
        assert!(!back.is_success());
    }
}
