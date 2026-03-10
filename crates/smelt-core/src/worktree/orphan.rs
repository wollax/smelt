//! Orphan detection logic for worktree sessions.
//!
//! Identifies worktrees whose owning process has died, whose state has gone
//! stale, or whose git worktree entry has disappeared.

use chrono::{Duration, Utc};

use super::state::{GitWorktreeEntry, SessionStatus, WorktreeState};

/// Default staleness threshold for orphan detection (24 hours).
pub const DEFAULT_STALENESS_HOURS: i64 = 24;

/// Check if a process with the given PID is alive.
///
/// Returns `true` if the process exists and we have permission to signal it.
/// Returns `false` for PIDs that exceed `i32::MAX` (invalid on POSIX).
pub fn is_pid_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: kill(pid, 0) sends no signal, only checks if the process exists.
    // This is a standard POSIX pattern.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Determine whether a worktree session is likely orphaned.
///
/// A session is considered orphaned if it is in `Running` status AND any of:
/// - Its PID is set and the process is no longer alive
/// - Its `updated_at` timestamp is older than `staleness_threshold`
/// - Its `worktree_path` has no matching entry in `git_worktrees` (state desync)
///
/// Sessions in `Created`, `Completed`, `Failed`, or `Orphaned` status are
/// never considered newly orphaned by this function.
pub fn is_likely_orphan(
    state: &WorktreeState,
    git_worktrees: &[GitWorktreeEntry],
    staleness_threshold: Duration,
    repo_root: &std::path::Path,
) -> bool {
    // Only Running sessions can become orphaned
    if state.status != SessionStatus::Running {
        return false;
    }

    // Check 1: PID is set and not alive
    if let Some(pid) = state.pid
        && !is_pid_alive(pid)
    {
        return true;
    }

    // Check 2: updated_at is older than staleness threshold
    let now = Utc::now();
    if now.signed_duration_since(state.updated_at) > staleness_threshold {
        return true;
    }

    // Check 3: state has a worktree_path but no matching entry in git worktrees
    let resolved_path = repo_root.join(&state.worktree_path);
    let has_git_entry = git_worktrees.iter().any(|entry| {
        match (entry.path.canonicalize(), resolved_path.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => entry.path == resolved_path,
        }
    });

    if !has_git_entry {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;

    fn make_state(status: SessionStatus, pid: Option<u32>, hours_ago: i64) -> WorktreeState {
        let now = Utc::now();
        let updated_at = now - Duration::hours(hours_ago);
        WorktreeState {
            session_name: "test-session".to_string(),
            branch_name: "smelt/test-session".to_string(),
            worktree_path: PathBuf::from("../repo-smelt-test-session"),
            base_ref: "HEAD".to_string(),
            status,
            created_at: updated_at,
            updated_at,
            pid,
            exit_code: None,
            task_description: None,
            file_scope: None,
        }
    }

    fn make_git_entry(path: &str, branch: &str) -> GitWorktreeEntry {
        GitWorktreeEntry {
            path: PathBuf::from(path),
            head: "abc1234".to_string(),
            branch: Some(branch.to_string()),
            is_bare: false,
            is_locked: false,
        }
    }

    #[test]
    fn is_pid_alive_with_current_process() {
        // Our own PID should always be alive
        let pid = std::process::id();
        assert!(is_pid_alive(pid));
    }

    #[test]
    fn is_pid_alive_with_very_large_pid() {
        // A very large PID is extremely unlikely to be alive
        assert!(!is_pid_alive(4_000_000));
    }

    #[test]
    fn created_session_is_not_orphan() {
        let state = make_state(SessionStatus::Created, None, 0);
        let threshold = Duration::hours(DEFAULT_STALENESS_HOURS);
        assert!(!is_likely_orphan(&state, &[], threshold, "/tmp".as_ref()));
    }

    #[test]
    fn completed_session_is_not_orphan() {
        let state = make_state(SessionStatus::Completed, None, 100);
        let threshold = Duration::hours(DEFAULT_STALENESS_HOURS);
        assert!(!is_likely_orphan(&state, &[], threshold, "/tmp".as_ref()));
    }

    #[test]
    fn failed_session_is_not_orphan() {
        let state = make_state(SessionStatus::Failed, None, 100);
        let threshold = Duration::hours(DEFAULT_STALENESS_HOURS);
        assert!(!is_likely_orphan(&state, &[], threshold, "/tmp".as_ref()));
    }

    #[test]
    fn running_session_with_dead_pid_is_orphan() {
        let state = make_state(SessionStatus::Running, Some(4_000_000), 0);
        // Provide a matching git entry so only PID check triggers
        let git_entries = vec![make_git_entry(
            "/tmp/../repo-smelt-test-session",
            "smelt/test-session",
        )];
        let threshold = Duration::hours(DEFAULT_STALENESS_HOURS);
        assert!(is_likely_orphan(
            &state,
            &git_entries,
            threshold,
            "/tmp".as_ref()
        ));
    }

    #[test]
    fn running_session_with_stale_timestamp_is_orphan() {
        // 48 hours old, threshold is 24 hours
        let state = make_state(SessionStatus::Running, None, 48);
        let git_entries = vec![make_git_entry(
            "/tmp/../repo-smelt-test-session",
            "smelt/test-session",
        )];
        let threshold = Duration::hours(DEFAULT_STALENESS_HOURS);
        assert!(is_likely_orphan(
            &state,
            &git_entries,
            threshold,
            "/tmp".as_ref()
        ));
    }

    #[test]
    fn running_session_without_git_entry_is_orphan() {
        // Recent, no PID, but no matching git worktree entry
        let state = make_state(SessionStatus::Running, None, 0);
        let threshold = Duration::hours(DEFAULT_STALENESS_HOURS);
        assert!(is_likely_orphan(&state, &[], threshold, "/tmp".as_ref()));
    }

    #[test]
    fn running_session_with_alive_pid_and_git_entry_is_not_orphan() {
        // Current process PID is alive, recent timestamp, has git entry
        let state = make_state(SessionStatus::Running, Some(std::process::id()), 0);
        let git_entries = vec![make_git_entry(
            "/tmp/../repo-smelt-test-session",
            "smelt/test-session",
        )];
        let threshold = Duration::hours(DEFAULT_STALENESS_HOURS);
        assert!(!is_likely_orphan(
            &state,
            &git_entries,
            threshold,
            "/tmp".as_ref()
        ));
    }
}
