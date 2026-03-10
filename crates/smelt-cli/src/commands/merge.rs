//! `smelt merge` command handler — run and plan subcommands.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Table};

use smelt_core::error::SmeltError;
use smelt_core::merge::conflict::ConflictScan;
use smelt_core::merge::{MergeOrderStrategy, MergePlan, MergeRunner};
use smelt_core::{
    ConflictAction, ConflictHandler, GitCli, Manifest, MergeOpts,
};

/// Subcommands for `smelt merge`.
#[derive(Subcommand)]
pub enum MergeCommands {
    /// Execute the merge pipeline
    Run {
        /// Path to the session manifest file
        manifest: String,
        /// Override target branch name
        #[arg(long)]
        target: Option<String>,
        /// Merge ordering strategy (completion-time or file-overlap)
        #[arg(long, value_parser = parse_strategy)]
        strategy: Option<MergeOrderStrategy>,
        /// Show full conflict context when conflicts occur
        #[arg(long)]
        verbose: bool,
    },
    /// Preview the merge order without executing
    Plan {
        /// Path to the session manifest file
        manifest: String,
        /// Override target branch name (used for display)
        #[arg(long)]
        target: Option<String>,
        /// Merge ordering strategy (completion-time or file-overlap)
        #[arg(long, value_parser = parse_strategy)]
        strategy: Option<MergeOrderStrategy>,
        /// Output as JSON instead of table
        #[arg(long)]
        json: bool,
    },
}

/// Parse a strategy string from the CLI into a `MergeOrderStrategy`.
fn parse_strategy(s: &str) -> Result<MergeOrderStrategy, String> {
    s.parse()
}

/// Interactive conflict handler that prompts the user via dialoguer on stderr.
struct InteractiveConflictHandler {
    verbose: bool,
}

impl ConflictHandler for InteractiveConflictHandler {
    async fn handle_conflict(
        &self,
        session_name: &str,
        files: &[String],
        scan: &ConflictScan,
        work_dir: &Path,
    ) -> smelt_core::Result<ConflictAction> {
        // If stderr is not a terminal, we cannot prompt interactively.
        // Propagate the conflict error (same behavior as NoopConflictHandler).
        if !console::Term::stderr().is_term() {
            return Err(SmeltError::MergeConflict {
                session: session_name.to_string(),
                files: files.to_vec(),
            });
        }

        eprintln!("\nConflict in session '{session_name}':");
        eprintln!("Conflicting files:");
        for file in files {
            eprintln!("  {file}");
        }

        if scan.has_markers {
            for hunk in &scan.hunks {
                eprintln!("  lines {}..{}", hunk.start_line, hunk.end_line);
            }

            if scan.total_conflict_lines < 20 {
                // Show inline conflict markers for small conflicts
                for file in files {
                    let path = work_dir.join(file);
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let file_scan =
                            smelt_core::merge::scan_conflict_markers(&content);
                        if file_scan.has_markers {
                            eprintln!("\n  --- {file} ---");
                            let lines: Vec<&str> = content.lines().collect();
                            for hunk in &file_scan.hunks {
                                for ln in hunk.start_line..=hunk.end_line {
                                    if ln <= lines.len() {
                                        let line = lines[ln - 1];
                                        let styled = if line.starts_with("<<<<<<<") {
                                            format!(
                                                "  {}",
                                                console::style(line).red()
                                            )
                                        } else if line.starts_with("=======") {
                                            format!(
                                                "  {}",
                                                console::style(line).yellow()
                                            )
                                        } else if line.starts_with(">>>>>>>") {
                                            format!(
                                                "  {}",
                                                console::style(line).green()
                                            )
                                        } else {
                                            format!("  {line}")
                                        };
                                        eprintln!("{styled}");
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                eprintln!(
                    "  ... {} conflict lines — edit externally",
                    scan.total_conflict_lines
                );
            }
        }

        if self.verbose {
            eprintln!("\nVerbose: conflict files in worktree at {}", work_dir.display());
            for file in files {
                let path = work_dir.join(file);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    eprintln!("\n  === {file} ===");
                    for line in content.lines() {
                        eprintln!("  {line}");
                    }
                }
            }
        }

        eprintln!("\nResolve conflicts in the files above, then choose an action:");

        let action = tokio::task::spawn_blocking(|| {
            let items = vec![
                "Resolve (conflicts fixed, continue)",
                "Skip (undo this session, continue others)",
                "Abort (stop merge, rollback)",
            ];
            let selection = dialoguer::Select::new()
                .with_prompt("Action")
                .items(&items)
                .default(0)
                .interact_on(&console::Term::stderr())
                .map_err(|e| SmeltError::SessionError {
                    session: String::new(),
                    message: format!("failed to read user input: {e}"),
                })?;
            Ok(match selection {
                0 => ConflictAction::Resolved,
                1 => ConflictAction::Skip,
                _ => ConflictAction::Abort,
            })
        })
        .await
        .map_err(|e| SmeltError::SessionError {
            session: session_name.to_string(),
            message: format!("prompt task failed: {e}"),
        })??;

        Ok(action)
    }
}

/// Execute the `smelt merge run` command.
///
/// Loads a manifest, runs the merge pipeline, prints progress to stderr
/// and summary diff stats to stdout.
pub async fn execute_merge_run(
    git: GitCli,
    repo_root: PathBuf,
    manifest_path: &str,
    target: Option<String>,
    strategy: Option<MergeOrderStrategy>,
    verbose: bool,
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
    let opts = MergeOpts::new(target, strategy, verbose);
    let handler = InteractiveConflictHandler { verbose };

    match runner.run(&manifest, opts, &handler).await {
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

            if let Some(ref plan) = report.plan {
                eprintln!(
                    "Strategy: {}{}",
                    plan.strategy,
                    if plan.fell_back {
                        " (fell back to completion-time)"
                    } else {
                        ""
                    }
                );
            }

            if report.has_skipped() {
                eprintln!(
                    "Skipped {} session(s): {}",
                    report.sessions_skipped.len(),
                    report.sessions_skipped.join(", ")
                );
            }

            if report.has_resolved() {
                eprintln!(
                    "Resolved {} session(s): {}",
                    report.sessions_resolved.len(),
                    report.sessions_resolved.join(", ")
                );
            }

            if report.has_conflict_skipped() {
                eprintln!(
                    "Skipped (conflict) {} session(s): {}",
                    report.sessions_conflict_skipped.len(),
                    report.sessions_conflict_skipped.join(", ")
                );
            }

            // Summary to stdout: per-session diff stats
            for result in &report.sessions_merged {
                println!("{}:", result.session_name);
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
        Err(SmeltError::MergeAborted { session }) => {
            eprintln!("Merge aborted by user during session '{session}'.");
            eprintln!("Target branch rolled back.");
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

/// Execute the `smelt merge plan` command.
///
/// Computes the merge order without executing, showing a table or JSON output.
pub async fn execute_merge_plan(
    git: GitCli,
    repo_root: PathBuf,
    manifest_path: &str,
    target: Option<String>,
    strategy: Option<MergeOrderStrategy>,
    json: bool,
) -> anyhow::Result<i32> {
    let manifest = match Manifest::load(std::path::Path::new(manifest_path)) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error: {e}");
            return Ok(1);
        }
    };

    let runner = MergeRunner::new(git, repo_root);
    let opts = MergeOpts::new(target, strategy, false);

    match runner.plan(&manifest, opts).await {
        Ok(plan) => {
            if json {
                let output =
                    serde_json::to_string_pretty(&plan).expect("MergePlan should serialize");
                println!("{output}");
            } else {
                print!("{}", format_plan_table(&plan));
            }
            Ok(0)
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

/// Format a `MergePlan` as a human-readable table string.
fn format_plan_table(plan: &MergePlan) -> String {
    let mut output = String::new();

    // Section 1: Merge Order
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec!["#", "Session", "Files Changed", "Strategy Note"]);

    for (i, entry) in plan.sessions.iter().enumerate() {
        let note = if i == 0 {
            "first".to_string()
        } else if plan.strategy == MergeOrderStrategy::FileOverlap && !plan.fell_back {
            "least overlap".to_string()
        } else {
            "manifest order".to_string()
        };

        table.add_row(vec![
            (i + 1).to_string(),
            entry.session_name.clone(),
            entry.file_count().to_string(),
            note,
        ]);
    }

    output.push_str("Merge Order\n");
    output.push_str(&table.to_string());
    output.push('\n');

    // Section 2: Pairwise overlaps (only for file-overlap strategy)
    if plan.strategy == MergeOrderStrategy::FileOverlap && !plan.pairwise_overlaps.is_empty() {
        output.push('\n');

        let mut overlap_table = Table::new();
        overlap_table.load_preset(UTF8_FULL);
        overlap_table.apply_modifier(UTF8_ROUND_CORNERS);
        overlap_table.set_header(vec![
            "Session A",
            "Session B",
            "Overlapping Files",
            "Count",
        ]);

        for overlap in &plan.pairwise_overlaps {
            let files_display = if overlap.overlapping_files.is_empty() {
                "(none)".to_string()
            } else {
                overlap.overlapping_files.join(", ")
            };

            overlap_table.add_row(vec![
                overlap.session_a.clone(),
                overlap.session_b.clone(),
                files_display,
                overlap.overlap_count().to_string(),
            ]);
        }

        output.push_str("Pairwise File Overlap\n");
        output.push_str(&overlap_table.to_string());
        output.push('\n');
    }

    // Section 3: Per-session file list
    output.push('\n');
    output.push_str("Session Files\n");
    for entry in &plan.sessions {
        output.push_str(&format!("  {}:\n", entry.session_name));
        let max_show = 10;
        for (i, file) in entry.changed_files.iter().enumerate() {
            if i >= max_show {
                output.push_str(&format!(
                    "    ... and {} more\n",
                    entry.changed_files.len() - max_show
                ));
                break;
            }
            output.push_str(&format!("    {file}\n"));
        }
        if entry.changed_files.is_empty() {
            output.push_str("    (no files)\n");
        }
    }

    // Fallback note
    if plan.fell_back {
        output.push('\n');
        output.push_str("Note: File overlap strategy could not differentiate sessions — fell back to manifest order.\n");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use smelt_core::merge::{PairwiseOverlap, SessionPlanEntry};

    #[test]
    fn test_parse_strategy_valid() {
        assert_eq!(
            parse_strategy("completion-time").unwrap(),
            MergeOrderStrategy::CompletionTime
        );
        assert_eq!(
            parse_strategy("file-overlap").unwrap(),
            MergeOrderStrategy::FileOverlap
        );
    }

    #[test]
    fn test_parse_strategy_invalid() {
        assert!(parse_strategy("random").is_err());
        assert!(parse_strategy("").is_err());
    }

    #[test]
    fn test_format_plan_table_renders() {
        let plan = MergePlan {
            strategy: MergeOrderStrategy::FileOverlap,
            fell_back: false,
            sessions: vec![
                SessionPlanEntry {
                    session_name: "alpha".to_string(),
                    branch_name: "smelt/alpha".to_string(),
                    changed_files: vec!["a.rs".to_string(), "shared.rs".to_string()],
                    original_index: 0,
                },
                SessionPlanEntry {
                    session_name: "beta".to_string(),
                    branch_name: "smelt/beta".to_string(),
                    changed_files: vec!["b.rs".to_string(), "shared.rs".to_string()],
                    original_index: 1,
                },
            ],
            pairwise_overlaps: vec![PairwiseOverlap {
                session_a: "alpha".to_string(),
                session_b: "beta".to_string(),
                overlapping_files: vec!["shared.rs".to_string()],
            }],
        };

        let output = format_plan_table(&plan);

        // Contains session names
        assert!(output.contains("alpha"));
        assert!(output.contains("beta"));

        // Contains overlap data
        assert!(output.contains("shared.rs"));

        // Contains table characters
        assert!(output.contains("│") || output.contains("─"));

        // Contains section headers
        assert!(output.contains("Merge Order"));
        assert!(output.contains("Pairwise File Overlap"));
        assert!(output.contains("Session Files"));

        // Not empty
        assert!(!output.is_empty());
    }

    #[test]
    fn test_format_plan_table_fallback_note() {
        let plan = MergePlan {
            strategy: MergeOrderStrategy::FileOverlap,
            fell_back: true,
            sessions: vec![SessionPlanEntry {
                session_name: "only".to_string(),
                branch_name: "smelt/only".to_string(),
                changed_files: vec!["file.rs".to_string()],
                original_index: 0,
            }],
            pairwise_overlaps: vec![],
        };

        let output = format_plan_table(&plan);
        assert!(output.contains("fell back to manifest order"));
    }

    #[test]
    fn test_format_plan_json_round_trip() {
        let plan = MergePlan {
            strategy: MergeOrderStrategy::FileOverlap,
            fell_back: false,
            sessions: vec![SessionPlanEntry {
                session_name: "alpha".to_string(),
                branch_name: "smelt/alpha".to_string(),
                changed_files: vec!["a.rs".to_string()],
                original_index: 0,
            }],
            pairwise_overlaps: vec![PairwiseOverlap {
                session_a: "alpha".to_string(),
                session_b: "beta".to_string(),
                overlapping_files: vec!["shared.rs".to_string()],
            }],
        };

        let json_str = serde_json::to_string_pretty(&plan).expect("serialize");

        // Contains expected keys
        assert!(json_str.contains("\"strategy\""));
        assert!(json_str.contains("\"sessions\""));
        assert!(json_str.contains("\"pairwise_overlaps\""));

        // Round-trip
        let deserialized: MergePlan = serde_json::from_str(&json_str).expect("deserialize");
        assert_eq!(deserialized.strategy, plan.strategy);
        assert_eq!(deserialized.fell_back, plan.fell_back);
        assert_eq!(deserialized.sessions.len(), plan.sessions.len());
        assert_eq!(
            deserialized.pairwise_overlaps.len(),
            plan.pairwise_overlaps.len()
        );
    }

    #[test]
    fn test_format_plan_table_many_files_truncated() {
        let files: Vec<String> = (0..15).map(|i| format!("file_{i}.rs")).collect();
        let plan = MergePlan {
            strategy: MergeOrderStrategy::CompletionTime,
            fell_back: false,
            sessions: vec![SessionPlanEntry {
                session_name: "big".to_string(),
                branch_name: "smelt/big".to_string(),
                changed_files: files,
                original_index: 0,
            }],
            pairwise_overlaps: vec![],
        };

        let output = format_plan_table(&plan);
        assert!(output.contains("... and 5 more"));
    }
}
