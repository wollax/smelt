//! Run state persistence, resume detection, and manifest hash computation.

use std::path::{Path, PathBuf};

use crate::orchestrate::types::{RunPhase, RunState};
use crate::summary::SummaryReport;

/// Compute a deterministic hash of manifest content for change detection.
///
/// Uses FNV-1a (stable across process invocations, unlike [`DefaultHasher`]
/// which uses SipHash with random seeds). Not cryptographic — sufficient for
/// detecting whether a manifest changed between runs.
pub fn compute_manifest_hash(manifest_content: &str) -> String {
    // FNV-1a 64-bit — deterministic, no external crate needed
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001B3;

    let mut hash = FNV_OFFSET;
    for byte in manifest_content.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

/// Manages run state persistence under `.smelt/runs/`.
pub struct RunStateManager {
    runs_dir: PathBuf,
}

impl RunStateManager {
    /// Create a new manager rooted at `smelt_dir/.../runs/`.
    pub fn new(smelt_dir: &Path) -> Self {
        Self {
            runs_dir: smelt_dir.join("runs"),
        }
    }

    /// Persist a [`RunState`] to `<runs_dir>/<run_id>/state.json`.
    ///
    /// Also creates the `logs/` subdirectory for session output capture.
    pub fn save_state(&self, state: &RunState) -> crate::Result<()> {
        let run_dir = self.runs_dir.join(&state.run_id);
        // RunState::save already creates the directory
        state.save(&run_dir)?;
        // Ensure logs directory exists
        let logs_dir = run_dir.join("logs");
        std::fs::create_dir_all(&logs_dir)
            .map_err(|e| crate::SmeltError::io("creating logs directory", &logs_dir, e))?;
        Ok(())
    }

    /// Load a [`RunState`] from `<runs_dir>/<run_id>/state.json`.
    pub fn load_state(&self, run_id: &str) -> crate::Result<RunState> {
        let run_dir = self.runs_dir.join(run_id);
        RunState::load(&run_dir)
    }

    /// Find the most recent resumable run for a given manifest name.
    ///
    /// Scans `.smelt/runs/` for directories starting with `<manifest_name>-`,
    /// loads each `state.json`, and returns the most recent one where
    /// [`RunState::is_resumable()`] is true.
    pub fn find_incomplete_run(&self, manifest_name: &str) -> crate::Result<Option<RunState>> {
        if !self.runs_dir.exists() {
            return Ok(None);
        }

        let prefix = format!("{manifest_name}-");
        let entries = std::fs::read_dir(&self.runs_dir)
            .map_err(|e| crate::SmeltError::io("reading runs directory", &self.runs_dir, e))?;

        let mut best: Option<RunState> = None;

        for entry in entries {
            let entry = entry.map_err(|e| {
                crate::SmeltError::io("reading runs directory entry", &self.runs_dir, e)
            })?;

            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.starts_with(&prefix) {
                continue;
            }

            // Try to load state; skip entries that fail to parse
            let state_path = entry.path().join("state.json");
            if !state_path.exists() {
                continue;
            }

            let state = match RunState::load(&entry.path()) {
                Ok(s) => s,
                Err(_) => continue,
            };

            if !state.is_resumable() {
                continue;
            }

            // Keep the most recent by updated_at
            let dominated = best
                .as_ref()
                .is_some_and(|b| b.updated_at >= state.updated_at);
            if !dominated {
                best = Some(state);
            }
        }

        Ok(best)
    }

    /// Return the log file path for a session within a run.
    pub fn log_path(&self, run_id: &str, session_name: &str) -> PathBuf {
        self.runs_dir
            .join(run_id)
            .join("logs")
            .join(format!("{session_name}.log"))
    }

    /// Persist a [`SummaryReport`] to `<runs_dir>/<run_id>/summary.json`.
    ///
    /// The summary file shares the run directory lifetime. Any future cleanup
    /// logic must preserve `summary.json` for standalone `smelt summary` access.
    pub fn save_summary(&self, run_id: &str, report: &SummaryReport) -> crate::Result<()> {
        let run_dir = self.runs_dir.join(run_id);
        std::fs::create_dir_all(&run_dir)
            .map_err(|e| crate::SmeltError::io("creating run directory for summary", &run_dir, e))?;

        let path = run_dir.join("summary.json");
        let json = serde_json::to_string_pretty(report).map_err(|e| {
            crate::SmeltError::Orchestration {
                message: format!("failed to serialize summary report: {e}"),
            }
        })?;

        std::fs::write(&path, json)
            .map_err(|e| crate::SmeltError::io("writing summary report", &path, e))?;

        Ok(())
    }

    /// Load a [`SummaryReport`] from `<runs_dir>/<run_id>/summary.json`.
    pub fn load_summary(&self, run_id: &str) -> crate::Result<SummaryReport> {
        let path = self.runs_dir.join(run_id).join("summary.json");
        let json = std::fs::read_to_string(&path)
            .map_err(|e| crate::SmeltError::io("reading summary report", &path, e))?;

        serde_json::from_str(&json).map_err(|e| crate::SmeltError::Orchestration {
            message: format!("failed to deserialize summary report: {e}"),
        })
    }

    /// Find the most recent completed run for a given manifest name.
    ///
    /// Scans `.smelt/runs/` for directories matching `<manifest_name>-*`,
    /// loads each `state.json`, finds ones where phase is `Complete`,
    /// and returns the `run_id` of the most recent (by `updated_at`).
    ///
    /// Returns `None` if no completed runs exist.
    pub fn find_latest_completed_run(
        &self,
        manifest_name: &str,
    ) -> crate::Result<Option<String>> {
        if !self.runs_dir.exists() {
            return Ok(None);
        }

        let prefix = format!("{manifest_name}-");
        let entries = std::fs::read_dir(&self.runs_dir)
            .map_err(|e| crate::SmeltError::io("reading runs directory", &self.runs_dir, e))?;

        let mut best: Option<(String, chrono::DateTime<chrono::Utc>)> = None;

        for entry in entries {
            let entry = entry.map_err(|e| {
                crate::SmeltError::io("reading runs directory entry", &self.runs_dir, e)
            })?;

            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.starts_with(&prefix) {
                continue;
            }

            let state_path = entry.path().join("state.json");
            if !state_path.exists() {
                continue;
            }

            let state = match RunState::load(&entry.path()) {
                Ok(s) => s,
                Err(_) => continue,
            };

            if state.phase != RunPhase::Complete {
                continue;
            }

            let dominated = best
                .as_ref()
                .is_some_and(|(_, ts)| *ts >= state.updated_at);
            if !dominated {
                best = Some((state.run_id.clone(), state.updated_at));
            }
        }

        Ok(best.map(|(id, _)| id))
    }

    /// Remove the entire run directory on successful completion.
    pub fn cleanup_completed_run(&self, run_id: &str) -> crate::Result<()> {
        let run_dir = self.runs_dir.join(run_id);
        if run_dir.exists() {
            std::fs::remove_dir_all(&run_dir)
                .map_err(|e| crate::SmeltError::io("cleaning up completed run", &run_dir, e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrate::types::{FailurePolicy, RunPhase, RunState};

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let smelt_dir = tmp.path().join(".smelt");
        std::fs::create_dir_all(&smelt_dir).unwrap();

        let manager = RunStateManager::new(&smelt_dir);
        let state = RunState::new(
            "test-run-20260310-120000".to_string(),
            "my-manifest".to_string(),
            "abc123".to_string(),
            FailurePolicy::SkipDependents,
            &["s1".to_string(), "s2".to_string()],
        );

        manager.save_state(&state).expect("save");
        let loaded = manager
            .load_state("test-run-20260310-120000")
            .expect("load");

        assert_eq!(loaded.run_id, "test-run-20260310-120000");
        assert_eq!(loaded.manifest_name, "my-manifest");
        assert_eq!(loaded.manifest_hash, "abc123");
        assert_eq!(loaded.failure_policy, FailurePolicy::SkipDependents);
        assert_eq!(loaded.sessions.len(), 2);

        // Verify logs directory was created
        let logs_dir = smelt_dir.join("runs/test-run-20260310-120000/logs");
        assert!(logs_dir.exists(), "logs directory should be created");
    }

    #[test]
    fn find_incomplete_run_returns_resumable() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let smelt_dir = tmp.path().join(".smelt");
        std::fs::create_dir_all(&smelt_dir).unwrap();

        let manager = RunStateManager::new(&smelt_dir);

        // Create a resumable run (Sessions phase)
        let state = RunState::new(
            "my-feature-20260310-120000".to_string(),
            "my-feature".to_string(),
            "hash1".to_string(),
            FailurePolicy::SkipDependents,
            &["s1".to_string()],
        );
        manager.save_state(&state).expect("save");

        let found = manager
            .find_incomplete_run("my-feature")
            .expect("find")
            .expect("should find resumable run");

        assert_eq!(found.run_id, "my-feature-20260310-120000");
        assert!(found.is_resumable());
    }

    #[test]
    fn find_incomplete_run_ignores_complete() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let smelt_dir = tmp.path().join(".smelt");
        std::fs::create_dir_all(&smelt_dir).unwrap();

        let manager = RunStateManager::new(&smelt_dir);

        // Create a completed run
        let mut state = RunState::new(
            "my-feature-20260310-120000".to_string(),
            "my-feature".to_string(),
            "hash1".to_string(),
            FailurePolicy::SkipDependents,
            &["s1".to_string()],
        );
        state.phase = RunPhase::Complete;
        manager.save_state(&state).expect("save");

        let found = manager.find_incomplete_run("my-feature").expect("find");
        assert!(found.is_none(), "completed run should not be returned");
    }

    #[test]
    fn compute_manifest_hash_deterministic() {
        let content = "name = \"test\"\nbase_ref = \"HEAD\"";
        let h1 = compute_manifest_hash(content);
        let h2 = compute_manifest_hash(content);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16, "hash should be 16 hex chars");
    }

    #[test]
    fn compute_manifest_hash_different_input() {
        let h1 = compute_manifest_hash("content A");
        let h2 = compute_manifest_hash("content B");
        assert_ne!(h1, h2);
    }

    #[test]
    fn cleanup_completed_run_removes_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let smelt_dir = tmp.path().join(".smelt");
        std::fs::create_dir_all(&smelt_dir).unwrap();

        let manager = RunStateManager::new(&smelt_dir);

        let state = RunState::new(
            "cleanup-test-20260310-120000".to_string(),
            "cleanup-test".to_string(),
            "hash".to_string(),
            FailurePolicy::SkipDependents,
            &["s1".to_string()],
        );
        manager.save_state(&state).expect("save");

        let run_dir = smelt_dir.join("runs/cleanup-test-20260310-120000");
        assert!(run_dir.exists());

        manager
            .cleanup_completed_run("cleanup-test-20260310-120000")
            .expect("cleanup");
        assert!(!run_dir.exists());
    }

    #[test]
    fn log_path_format() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let smelt_dir = tmp.path().join(".smelt");

        let manager = RunStateManager::new(&smelt_dir);
        let path = manager.log_path("run-123", "my-session");

        assert!(path.ends_with("runs/run-123/logs/my-session.log"));
    }

    #[test]
    fn find_incomplete_run_no_runs_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let smelt_dir = tmp.path().join(".smelt");
        // Don't create the runs directory
        std::fs::create_dir_all(&smelt_dir).unwrap();

        let manager = RunStateManager::new(&smelt_dir);
        let found = manager.find_incomplete_run("anything").expect("find");
        assert!(found.is_none());
    }

    #[test]
    fn save_and_load_summary_roundtrip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let smelt_dir = tmp.path().join(".smelt");
        std::fs::create_dir_all(&smelt_dir).unwrap();

        let manager = RunStateManager::new(&smelt_dir);

        let report = SummaryReport {
            manifest_name: "test-manifest".to_string(),
            run_id: "test-run-001".to_string(),
            base_ref: "main".to_string(),
            sessions: vec![crate::summary::SessionSummary {
                session_name: "alpha".to_string(),
                files: vec![crate::summary::FileStat {
                    path: "src/a.rs".to_string(),
                    insertions: 10,
                    deletions: 2,
                }],
                total_insertions: 10,
                total_deletions: 2,
                commit_messages: vec!["Add A".to_string()],
                violations: vec![],
            }],
            totals: crate::summary::SummaryTotals {
                sessions: 1,
                files_changed: 1,
                insertions: 10,
                deletions: 2,
                violations: 0,
            },
        };

        manager.save_summary("test-run-001", &report).expect("save");
        let loaded = manager.load_summary("test-run-001").expect("load");

        assert_eq!(loaded.manifest_name, "test-manifest");
        assert_eq!(loaded.run_id, "test-run-001");
        assert_eq!(loaded.base_ref, "main");
        assert_eq!(loaded.sessions.len(), 1);
        assert_eq!(loaded.sessions[0].session_name, "alpha");
        assert_eq!(loaded.sessions[0].total_insertions, 10);
        assert_eq!(loaded.totals.files_changed, 1);
    }

    #[test]
    fn find_latest_completed_run_returns_newest() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let smelt_dir = tmp.path().join(".smelt");
        std::fs::create_dir_all(&smelt_dir).unwrap();

        let manager = RunStateManager::new(&smelt_dir);

        // Create two completed runs with different timestamps
        let mut state1 = RunState::new(
            "my-feat-20260310-100000".to_string(),
            "my-feat".to_string(),
            "h1".to_string(),
            FailurePolicy::SkipDependents,
            &["s1".to_string()],
        );
        state1.phase = RunPhase::Complete;
        manager.save_state(&state1).expect("save state1");

        // Small delay to ensure different updated_at
        std::thread::sleep(std::time::Duration::from_millis(10));

        let mut state2 = RunState::new(
            "my-feat-20260310-110000".to_string(),
            "my-feat".to_string(),
            "h2".to_string(),
            FailurePolicy::SkipDependents,
            &["s1".to_string()],
        );
        state2.phase = RunPhase::Complete;
        manager.save_state(&state2).expect("save state2");

        let found = manager
            .find_latest_completed_run("my-feat")
            .expect("find")
            .expect("should find completed run");

        assert_eq!(found, "my-feat-20260310-110000");
    }

    #[test]
    fn find_latest_completed_run_ignores_incomplete() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let smelt_dir = tmp.path().join(".smelt");
        std::fs::create_dir_all(&smelt_dir).unwrap();

        let manager = RunStateManager::new(&smelt_dir);

        // Create an incomplete run (Sessions phase)
        let state = RunState::new(
            "my-feat-20260310-120000".to_string(),
            "my-feat".to_string(),
            "h1".to_string(),
            FailurePolicy::SkipDependents,
            &["s1".to_string()],
        );
        manager.save_state(&state).expect("save");

        let found = manager
            .find_latest_completed_run("my-feat")
            .expect("find");
        assert!(found.is_none(), "incomplete run should not be returned");
    }
}
