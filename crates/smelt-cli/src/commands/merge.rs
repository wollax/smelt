//! `smelt merge` command handler.

use std::path::PathBuf;

use smelt_core::error::SmeltError;
use smelt_core::merge::MergeRunner;
use smelt_core::{GitCli, Manifest, MergeOpts};

/// Execute the `smelt merge` command.
///
/// Loads a manifest, runs the merge pipeline, prints progress to stderr
/// and summary diff stats to stdout.
pub async fn execute_merge(
    git: GitCli,
    repo_root: PathBuf,
    manifest_path: &str,
    target: Option<String>,
) -> anyhow::Result<i32> {
    let manifest = match Manifest::load(std::path::Path::new(manifest_path)) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error: {e}");
            return Ok(1);
        }
    };

    eprintln!(
        "Merging sessions from manifest '{}'...",
        manifest.manifest.name
    );

    let runner = MergeRunner::new(git, repo_root);
    let opts = match target {
        Some(branch) => MergeOpts::with_target_branch(branch),
        None => MergeOpts::default(),
    };

    match runner.run(&manifest, opts).await {
        Ok(report) => {
            // Progress summary to stderr
            for (i, result) in report.sessions_merged.iter().enumerate() {
                eprintln!(
                    "[{}/{}] Merged '{}'",
                    i + 1,
                    report.sessions_merged.len(),
                    result.session_name
                );
            }

            eprintln!(
                "Merged {} session(s) into '{}'",
                report.sessions_merged.len(),
                report.target_branch
            );

            if report.has_skipped() {
                eprintln!(
                    "Skipped {} session(s): {}",
                    report.sessions_skipped.len(),
                    report.sessions_skipped.join(", ")
                );
            }

            // Summary to stdout: per-session diff stats
            for result in &report.sessions_merged {
                println!(
                    "{}:",
                    result.session_name
                );
                println!(
                    "  {} file(s) changed, {} insertion(s), {} deletion(s)",
                    result.files_changed, result.insertions, result.deletions
                );
            }

            Ok(0)
        }
        Err(SmeltError::MergeConflict { session, files }) => {
            eprintln!("Error: merge conflict in session '{session}'");
            eprintln!("Conflicting files:");
            for f in &files {
                eprintln!("  {f}");
            }
            eprintln!("Target branch rolled back. Session worktrees preserved for inspection.");
            Ok(1)
        }
        Err(e @ SmeltError::MergeTargetExists { .. }) => {
            eprintln!("Error: {e}");
            Ok(1)
        }
        Err(SmeltError::NoCompletedSessions) => {
            eprintln!("Error: {}", SmeltError::NoCompletedSessions);
            Ok(1)
        }
        Err(SmeltError::NotInitialized) => {
            eprintln!("Error: {}", SmeltError::NotInitialized);
            Ok(1)
        }
        Err(SmeltError::SessionError { session, message }) => {
            eprintln!("Error: session '{session}': {message}");
            Ok(1)
        }
        Err(e) => {
            eprintln!("Error: {e}");
            Ok(1)
        }
    }
}
