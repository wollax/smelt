//! Summary data collection — gathers per-session diff stats, commit messages,
//! and scope violations.

use std::collections::HashMap;

use tracing::warn;

use crate::git::GitOps;
use crate::orchestrate::types::SessionRunState;
use crate::session::manifest::Manifest;
use crate::summary::scope::check_scope;
use crate::summary::types::{FileStat, SessionSummary, SummaryReport, SummaryTotals};

/// Derive session branch name from session name.
///
/// Duplicates the naming convention from [`crate::worktree::WorktreeManager`].
/// If the worktree branch naming convention changes, this must be updated too.
fn session_branch_name(session_name: &str) -> String {
    format!("smelt/{session_name}")
}

/// Collect a summary report for all completed sessions in a manifest.
///
/// For each session, gathers:
/// - Files changed with line counts (via `diff_numstat` + `diff_name_only`)
/// - Commit messages (via `log_subjects`)
/// - Scope violations (via `check_scope`)
///
/// Sessions missing from `session_states` or not in the `Completed` state are skipped.
/// If git operations fail for a particular session (e.g. branch does not exist),
/// that session is skipped with a warning rather than failing the entire operation.
pub async fn collect_summary<G: GitOps>(
    git: &G,
    manifest: &Manifest,
    session_states: &HashMap<String, SessionRunState>,
    run_id: &str,
) -> crate::Result<SummaryReport> {
    let manifest_base_ref = &manifest.manifest.base_ref;
    let shared_files = &manifest.manifest.shared_files;
    let mut session_summaries = Vec::new();

    for session_def in &manifest.sessions {
        let session_name = &session_def.name;

        // Skip sessions that are not completed
        let Some(state) = session_states.get(session_name) else {
            continue;
        };
        if !state.is_success() {
            continue;
        }

        // Determine base_ref: session override or manifest default
        let base_ref = session_def
            .base_ref
            .as_deref()
            .unwrap_or(manifest_base_ref);

        let session_branch = session_branch_name(session_name);

        // Gather diff_numstat
        let numstat = match git.diff_numstat(base_ref, &session_branch).await {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    "Failed to collect diff_numstat for session '{session_name}': {e}"
                );
                continue;
            }
        };

        // Gather diff_name_only (catches binary files that numstat shows as - -)
        let name_only = match git.diff_name_only(base_ref, &session_branch).await {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    "Failed to collect diff_name_only for session '{session_name}': {e}"
                );
                continue;
            }
        };

        // Gather log subjects
        let commit_messages =
            match git.log_subjects(&format!("{base_ref}..{session_branch}")).await {
                Ok(v) => v,
                Err(e) => {
                    warn!(
                        "Failed to collect log_subjects for session '{session_name}': {e}"
                    );
                    continue;
                }
            };

        // Merge numstat and name_only results
        let mut numstat_map: HashMap<&str, (usize, usize)> = HashMap::new();
        for (ins, del, path) in &numstat {
            numstat_map.insert(path.as_str(), (*ins, *del));
        }

        let mut files: Vec<FileStat> = Vec::new();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

        // First, add all files from name_only (canonical list)
        for path in &name_only {
            seen.insert(path.as_str());
            let (insertions, deletions) = numstat_map
                .get(path.as_str())
                .copied()
                .unwrap_or((0, 0));
            files.push(FileStat {
                path: path.clone(),
                insertions,
                deletions,
            });
        }

        // Add any files from numstat that weren't in name_only.
        // Unreachable with well-formed git output; kept as a safety net.
        for (ins, del, path) in &numstat {
            if !seen.contains(path.as_str()) {
                files.push(FileStat {
                    path: path.clone(),
                    insertions: *ins,
                    deletions: *del,
                });
            }
        }

        // Compute totals
        let total_insertions: usize = files.iter().map(|f| f.insertions).sum();
        let total_deletions: usize = files.iter().map(|f| f.deletions).sum();

        // Compute scope violations
        let changed_paths: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
        let violations = check_scope(
            session_name,
            session_def.file_scope.as_deref(),
            shared_files,
            &changed_paths,
        );

        session_summaries.push(SessionSummary {
            session_name: session_name.clone(),
            files,
            total_insertions,
            total_deletions,
            commit_messages,
            violations,
        });
    }

    // Compute totals across all session summaries
    let totals = SummaryTotals {
        sessions: session_summaries.len(),
        files_changed: session_summaries.iter().map(|s| s.files.len()).sum(),
        insertions: session_summaries.iter().map(|s| s.total_insertions).sum(),
        deletions: session_summaries.iter().map(|s| s.total_deletions).sum(),
        violations: session_summaries
            .iter()
            .map(|s| s.violations.len())
            .sum(),
    };

    Ok(SummaryReport {
        manifest_name: manifest.manifest.name.clone(),
        run_id: run_id.to_string(),
        base_ref: manifest_base_ref.clone(),
        sessions: session_summaries,
        totals,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::GitOps;
    use crate::worktree::GitWorktreeEntry;
    use std::path::{Path, PathBuf};

    /// Minimal mock implementing only the GitOps methods needed by collect_summary.
    struct MockGitOps {
        numstat_results: HashMap<String, Vec<(usize, usize, String)>>,
        name_only_results: HashMap<String, Vec<String>>,
        log_results: HashMap<String, Vec<String>>,
    }

    impl MockGitOps {
        fn new() -> Self {
            Self {
                numstat_results: HashMap::new(),
                name_only_results: HashMap::new(),
                log_results: HashMap::new(),
            }
        }

        /// Register diff_numstat result for a given (base, head) pair.
        fn add_numstat(
            &mut self,
            base: &str,
            head: &str,
            results: Vec<(usize, usize, &str)>,
        ) {
            let key = format!("{base}..{head}");
            self.numstat_results.insert(
                key,
                results
                    .into_iter()
                    .map(|(i, d, p)| (i, d, p.to_string()))
                    .collect(),
            );
        }

        /// Register diff_name_only result for a given (base, head) pair.
        fn add_name_only(&mut self, base: &str, head: &str, results: Vec<&str>) {
            let key = format!("{base}..{head}");
            self.name_only_results
                .insert(key, results.into_iter().map(String::from).collect());
        }

        /// Register log_subjects result for a given range.
        fn add_log_subjects(&mut self, range: &str, results: Vec<&str>) {
            self.log_results
                .insert(range.to_string(), results.into_iter().map(String::from).collect());
        }
    }

    impl GitOps for MockGitOps {
        async fn diff_numstat(
            &self,
            from_ref: &str,
            to_ref: &str,
        ) -> crate::Result<Vec<(usize, usize, String)>> {
            let key = format!("{from_ref}..{to_ref}");
            self.numstat_results
                .get(&key)
                .cloned()
                .ok_or_else(|| crate::SmeltError::Orchestration {
                    message: format!("no mock numstat for {key}"),
                })
        }

        async fn diff_name_only(
            &self,
            base_ref: &str,
            head_ref: &str,
        ) -> crate::Result<Vec<String>> {
            let key = format!("{base_ref}..{head_ref}");
            self.name_only_results
                .get(&key)
                .cloned()
                .ok_or_else(|| crate::SmeltError::Orchestration {
                    message: format!("no mock name_only for {key}"),
                })
        }

        async fn log_subjects(&self, range: &str) -> crate::Result<Vec<String>> {
            self.log_results
                .get(range)
                .cloned()
                .ok_or_else(|| crate::SmeltError::Orchestration {
                    message: format!("no mock log_subjects for {range}"),
                })
        }

        // -- Unimplemented methods below --

        async fn repo_root(&self) -> crate::Result<PathBuf> {
            unimplemented!()
        }
        async fn is_inside_work_tree(&self, _: &Path) -> crate::Result<bool> {
            unimplemented!()
        }
        async fn current_branch(&self) -> crate::Result<String> {
            unimplemented!()
        }
        async fn head_short(&self) -> crate::Result<String> {
            unimplemented!()
        }
        async fn worktree_add(
            &self,
            _: &Path,
            _: &str,
            _: &str,
        ) -> crate::Result<()> {
            unimplemented!()
        }
        async fn worktree_remove(&self, _: &Path, _: bool) -> crate::Result<()> {
            unimplemented!()
        }
        async fn worktree_list(&self) -> crate::Result<Vec<GitWorktreeEntry>> {
            unimplemented!()
        }
        async fn worktree_prune(&self) -> crate::Result<()> {
            unimplemented!()
        }
        async fn worktree_is_dirty(&self, _: &Path) -> crate::Result<bool> {
            unimplemented!()
        }
        async fn branch_delete(&self, _: &str, _: bool) -> crate::Result<()> {
            unimplemented!()
        }
        async fn branch_is_merged(&self, _: &str, _: &str) -> crate::Result<bool> {
            unimplemented!()
        }
        async fn branch_exists(&self, _: &str) -> crate::Result<bool> {
            unimplemented!()
        }
        async fn add(&self, _: &Path, _: &[&str]) -> crate::Result<()> {
            unimplemented!()
        }
        async fn commit(&self, _: &Path, _: &str) -> crate::Result<String> {
            unimplemented!()
        }
        async fn rev_list_count(&self, _: &str, _: &str) -> crate::Result<usize> {
            unimplemented!()
        }
        async fn merge_base(&self, _: &str, _: &str) -> crate::Result<String> {
            unimplemented!()
        }
        async fn branch_create(&self, _: &str, _: &str) -> crate::Result<()> {
            unimplemented!()
        }
        async fn merge_squash(&self, _: &Path, _: &str) -> crate::Result<()> {
            unimplemented!()
        }
        async fn worktree_add_existing(&self, _: &Path, _: &str) -> crate::Result<()> {
            unimplemented!()
        }
        async fn unmerged_files(&self, _: &Path) -> crate::Result<Vec<String>> {
            unimplemented!()
        }
        async fn reset_hard(&self, _: &Path, _: &str) -> crate::Result<()> {
            unimplemented!()
        }
        async fn rev_parse(&self, _: &str) -> crate::Result<String> {
            unimplemented!()
        }
        async fn show_index_stage(
            &self,
            _: &Path,
            _: u8,
            _: &str,
        ) -> crate::Result<String> {
            unimplemented!()
        }
    }

    fn make_manifest(name: &str, base_ref: &str, sessions: Vec<crate::session::manifest::SessionDef>) -> Manifest {
        Manifest {
            manifest: crate::session::manifest::ManifestMeta {
                name: name.to_string(),
                base_ref: base_ref.to_string(),
                merge_strategy: None,
                parallel_by_default: true,
                on_failure: None,
                shared_files: vec![],
            },
            sessions,
        }
    }

    fn session_def(name: &str, file_scope: Option<Vec<&str>>, base_ref: Option<&str>) -> crate::session::manifest::SessionDef {
        crate::session::manifest::SessionDef {
            name: name.to_string(),
            task: Some("test task".to_string()),
            task_file: None,
            file_scope: file_scope.map(|v| v.into_iter().map(String::from).collect()),
            base_ref: base_ref.map(String::from),
            timeout_secs: None,
            env: None,
            depends_on: None,
            script: None,
        }
    }

    #[tokio::test]
    async fn collect_summary_basic() {
        let mut git = MockGitOps::new();

        // Session alpha
        git.add_numstat("main", "smelt/alpha", vec![(10, 2, "src/a.rs"), (5, 0, "src/b.rs")]);
        git.add_name_only("main", "smelt/alpha", vec!["src/a.rs", "src/b.rs"]);
        git.add_log_subjects("main..smelt/alpha", vec!["Add module A", "Fix bug"]);

        // Session beta
        git.add_numstat("main", "smelt/beta", vec![(3, 1, "src/c.rs")]);
        git.add_name_only("main", "smelt/beta", vec!["src/c.rs"]);
        git.add_log_subjects("main..smelt/beta", vec!["Add module C"]);

        let manifest = make_manifest(
            "test",
            "main",
            vec![session_def("alpha", None, None), session_def("beta", None, None)],
        );

        let mut states = HashMap::new();
        states.insert("alpha".to_string(), SessionRunState::Completed { duration_secs: 1.0 });
        states.insert("beta".to_string(), SessionRunState::Completed { duration_secs: 2.0 });

        let report = collect_summary(&git, &manifest, &states, "run-1").await.unwrap();

        assert_eq!(report.manifest_name, "test");
        assert_eq!(report.run_id, "run-1");
        assert_eq!(report.base_ref, "main");
        assert_eq!(report.sessions.len(), 2);

        let alpha = &report.sessions[0];
        assert_eq!(alpha.session_name, "alpha");
        assert_eq!(alpha.files.len(), 2);
        assert_eq!(alpha.total_insertions, 15);
        assert_eq!(alpha.total_deletions, 2);
        assert_eq!(alpha.commit_messages, vec!["Add module A", "Fix bug"]);

        let beta = &report.sessions[1];
        assert_eq!(beta.session_name, "beta");
        assert_eq!(beta.files.len(), 1);
        assert_eq!(beta.total_insertions, 3);
        assert_eq!(beta.total_deletions, 1);

        // Totals
        assert_eq!(report.totals.sessions, 2);
        assert_eq!(report.totals.files_changed, 3);
        assert_eq!(report.totals.insertions, 18);
        assert_eq!(report.totals.deletions, 3);
        assert_eq!(report.totals.violations, 0);
    }

    #[tokio::test]
    async fn collect_summary_skips_non_completed() {
        let mut git = MockGitOps::new();

        // Only set up git data for the completed session
        git.add_numstat("main", "smelt/good", vec![(1, 0, "ok.rs")]);
        git.add_name_only("main", "smelt/good", vec!["ok.rs"]);
        git.add_log_subjects("main..smelt/good", vec!["Working"]);

        let manifest = make_manifest(
            "test",
            "main",
            vec![session_def("good", None, None), session_def("bad", None, None)],
        );

        let mut states = HashMap::new();
        states.insert("good".to_string(), SessionRunState::Completed { duration_secs: 1.0 });
        states.insert("bad".to_string(), SessionRunState::Failed { reason: "boom".to_string() });

        let report = collect_summary(&git, &manifest, &states, "run-2").await.unwrap();

        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.sessions[0].session_name, "good");
        assert_eq!(report.totals.sessions, 1);
    }

    #[tokio::test]
    async fn collect_summary_binary_files() {
        let mut git = MockGitOps::new();

        // diff_numstat returns 0,0 for binary files (git shows - - which gets parsed as 0,0)
        // diff_name_only includes the binary file
        git.add_numstat("main", "smelt/media", vec![(5, 1, "src/lib.rs")]);
        git.add_name_only("main", "smelt/media", vec!["src/lib.rs", "assets/logo.png"]);
        git.add_log_subjects("main..smelt/media", vec!["Add logo"]);

        let manifest = make_manifest("test", "main", vec![session_def("media", None, None)]);

        let mut states = HashMap::new();
        states.insert("media".to_string(), SessionRunState::Completed { duration_secs: 1.0 });

        let report = collect_summary(&git, &manifest, &states, "run-3").await.unwrap();

        assert_eq!(report.sessions[0].files.len(), 2);
        let binary_file = report.sessions[0]
            .files
            .iter()
            .find(|f| f.path == "assets/logo.png")
            .expect("binary file should be present");
        assert_eq!(binary_file.insertions, 0);
        assert_eq!(binary_file.deletions, 0);

        // Totals should include text file stats but binary is 0/0
        assert_eq!(report.totals.insertions, 5);
        assert_eq!(report.totals.deletions, 1);
        assert_eq!(report.totals.files_changed, 2);
    }

    #[tokio::test]
    async fn collect_summary_with_scope_violations() {
        let mut git = MockGitOps::new();

        git.add_numstat("main", "smelt/auth", vec![
            (10, 0, "src/auth/login.rs"),
            (3, 0, "src/db/schema.rs"),
        ]);
        git.add_name_only("main", "smelt/auth", vec![
            "src/auth/login.rs",
            "src/db/schema.rs",
        ]);
        git.add_log_subjects("main..smelt/auth", vec!["Add login"]);

        let manifest = make_manifest(
            "test",
            "main",
            vec![session_def("auth", Some(vec!["src/auth/**"]), None)],
        );

        let mut states = HashMap::new();
        states.insert("auth".to_string(), SessionRunState::Completed { duration_secs: 1.0 });

        let report = collect_summary(&git, &manifest, &states, "run-4").await.unwrap();

        assert_eq!(report.sessions[0].violations.len(), 1);
        assert_eq!(report.sessions[0].violations[0].file_path, "src/db/schema.rs");
        assert_eq!(report.totals.violations, 1);
        assert!(report.has_violations());
    }

    #[tokio::test]
    async fn collect_summary_shared_files_no_violations() {
        let mut git = MockGitOps::new();

        git.add_numstat("main", "smelt/auth", vec![
            (10, 0, "src/auth/login.rs"),
            (1, 0, "Cargo.toml"),
        ]);
        git.add_name_only("main", "smelt/auth", vec![
            "src/auth/login.rs",
            "Cargo.toml",
        ]);
        git.add_log_subjects("main..smelt/auth", vec!["Add login"]);

        let mut manifest = make_manifest(
            "test",
            "main",
            vec![session_def("auth", Some(vec!["src/auth/**"]), None)],
        );
        manifest.manifest.shared_files = vec!["Cargo.toml".to_string(), "Cargo.lock".to_string()];

        let mut states = HashMap::new();
        states.insert("auth".to_string(), SessionRunState::Completed { duration_secs: 1.0 });

        let report = collect_summary(&git, &manifest, &states, "run-5").await.unwrap();

        assert!(report.sessions[0].violations.is_empty());
        assert_eq!(report.totals.violations, 0);
        assert!(!report.has_violations());
    }

    #[tokio::test]
    async fn collect_summary_respects_session_base_ref_override() {
        let mut git = MockGitOps::new();

        // Session with base_ref override uses "develop" instead of manifest "main"
        git.add_numstat("develop", "smelt/feature", vec![(2, 0, "x.rs")]);
        git.add_name_only("develop", "smelt/feature", vec!["x.rs"]);
        git.add_log_subjects("develop..smelt/feature", vec!["Feature"]);

        let manifest = make_manifest(
            "test",
            "main",
            vec![session_def("feature", None, Some("develop"))],
        );

        let mut states = HashMap::new();
        states.insert("feature".to_string(), SessionRunState::Completed { duration_secs: 1.0 });

        let report = collect_summary(&git, &manifest, &states, "run-6").await.unwrap();

        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.sessions[0].files[0].path, "x.rs");
    }
}
