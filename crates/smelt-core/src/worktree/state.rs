//! Worktree state types for Smelt session tracking.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Status of a worktree session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Created,
    Running,
    Completed,
    Failed,
    Orphaned,
}

/// Per-session worktree metadata, serialized to `.smelt/worktrees/<name>.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeState {
    pub session_name: String,
    pub branch_name: String,
    pub worktree_path: PathBuf,
    pub base_ref: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub task_description: Option<String>,
    pub file_scope: Option<Vec<String>>,
}

/// Entry from `git worktree list --porcelain`.
#[derive(Debug, Clone)]
pub struct GitWorktreeEntry {
    pub path: PathBuf,
    pub head: String,
    pub branch: Option<String>,
    pub is_bare: bool,
    pub is_locked: bool,
}

/// Parse the output of `git worktree list --porcelain` into structured entries.
pub fn parse_porcelain(output: &str) -> Vec<GitWorktreeEntry> {
    let mut entries = Vec::new();
    let mut path: Option<String> = None;
    let mut head: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut is_bare = false;
    let mut is_locked = false;

    for line in output.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            // If we already have a pending entry, push it
            if let (Some(p_val), Some(h_val)) = (path.take(), head.take()) {
                entries.push(GitWorktreeEntry {
                    path: PathBuf::from(p_val),
                    head: h_val,
                    branch: branch.take(),
                    is_bare,
                    is_locked,
                });
                is_bare = false;
                is_locked = false;
            }
            path = Some(p.to_string());
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            head = Some(h.to_string());
        } else if let Some(b) = line.strip_prefix("branch ") {
            branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
        } else if line == "bare" {
            is_bare = true;
        } else if line == "locked" || line.starts_with("locked ") {
            is_locked = true;
        } else if line.is_empty()
            && let (Some(p_val), Some(h_val)) = (path.take(), head.take())
        {
            entries.push(GitWorktreeEntry {
                path: PathBuf::from(p_val),
                head: h_val,
                branch: branch.take(),
                is_bare,
                is_locked,
            });
            is_bare = false;
            is_locked = false;
        }
    }

    // Handle last entry if no trailing blank line
    if let (Some(p_val), Some(h_val)) = (path.take(), head.take()) {
        entries.push(GitWorktreeEntry {
            path: PathBuf::from(p_val),
            head: h_val,
            branch: branch.take(),
            is_bare,
            is_locked,
        });
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_status_round_trip_serde() {
        // TOML requires a top-level table, so wrap the status in a struct for round-trip.
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Wrapper {
            status: SessionStatus,
        }

        for status in [
            SessionStatus::Created,
            SessionStatus::Running,
            SessionStatus::Completed,
            SessionStatus::Failed,
            SessionStatus::Orphaned,
        ] {
            let wrapper = Wrapper {
                status: status.clone(),
            };
            let serialized = toml::to_string(&wrapper).unwrap();
            let deserialized: Wrapper = toml::from_str(&serialized).unwrap();
            assert_eq!(wrapper, deserialized);
        }
    }

    #[test]
    fn worktree_state_round_trip_toml() {
        let state = WorktreeState {
            session_name: "add-auth-flow".to_string(),
            branch_name: "smelt/add-auth-flow".to_string(),
            worktree_path: PathBuf::from("../myrepo-smelt-add-auth-flow"),
            base_ref: "main".to_string(),
            status: SessionStatus::Created,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            pid: Some(12345),
            exit_code: None,
            task_description: Some("Implement authentication flow".to_string()),
            file_scope: Some(vec!["src/auth.rs".to_string(), "src/lib.rs".to_string()]),
        };

        let toml_str = toml::to_string(&state).expect("serialize to TOML");
        let deserialized: WorktreeState =
            toml::from_str(&toml_str).expect("deserialize from TOML");

        assert_eq!(state.session_name, deserialized.session_name);
        assert_eq!(state.branch_name, deserialized.branch_name);
        assert_eq!(state.worktree_path, deserialized.worktree_path);
        assert_eq!(state.base_ref, deserialized.base_ref);
        assert_eq!(state.status, deserialized.status);
        assert_eq!(state.pid, deserialized.pid);
        assert_eq!(state.exit_code, deserialized.exit_code);
        assert_eq!(state.task_description, deserialized.task_description);
        assert_eq!(state.file_scope, deserialized.file_scope);
    }

    #[test]
    fn parse_porcelain_normal_worktrees() {
        let output = "\
worktree /home/user/project
HEAD abc1234567890abcdef1234567890abcdef123456
branch refs/heads/main

worktree /home/user/project-wt
HEAD def4567890abcdef1234567890abcdef12345678
branch refs/heads/feature/auth

";

        let entries = parse_porcelain(output);
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].path, PathBuf::from("/home/user/project"));
        assert_eq!(
            entries[0].head,
            "abc1234567890abcdef1234567890abcdef123456"
        );
        assert_eq!(entries[0].branch.as_deref(), Some("main"));
        assert!(!entries[0].is_bare);
        assert!(!entries[0].is_locked);

        assert_eq!(entries[1].path, PathBuf::from("/home/user/project-wt"));
        assert_eq!(entries[1].branch.as_deref(), Some("feature/auth"));
    }

    #[test]
    fn parse_porcelain_bare_repo() {
        let output = "\
worktree /home/user/bare-repo
HEAD abc1234567890abcdef1234567890abcdef123456
bare

";

        let entries = parse_porcelain(output);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_bare);
        assert!(entries[0].branch.is_none());
    }

    #[test]
    fn parse_porcelain_detached_head() {
        let output = "\
worktree /home/user/project
HEAD abc1234567890abcdef1234567890abcdef123456
branch refs/heads/main

worktree /home/user/project-detached
HEAD def4567890abcdef1234567890abcdef12345678
detached

";

        let entries = parse_porcelain(output);
        assert_eq!(entries.len(), 2);
        assert!(entries[1].branch.is_none());
        assert!(!entries[1].is_bare);
    }

    #[test]
    fn parse_porcelain_no_trailing_newline() {
        let output = "\
worktree /home/user/project
HEAD abc1234567890abcdef1234567890abcdef123456
branch refs/heads/main";

        let entries = parse_porcelain(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].branch.as_deref(), Some("main"));
    }
}
