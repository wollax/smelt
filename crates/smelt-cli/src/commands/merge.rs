//! `smelt merge` command handler — run and plan subcommands.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Subcommand;
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Table};
use similar::TextDiff;

use smelt_core::ai::{
    AiConfig, AiProvider, GenAiProvider, build_resolution_prompt, build_retry_prompt,
    build_system_prompt,
};
use smelt_core::error::SmeltError;
use smelt_core::merge::conflict::ConflictScan;
use smelt_core::merge::{
    AiConflictHandler, MergeOrderStrategy, MergePlan, MergeRunner, default_model_for_provider,
};
use smelt_core::{
    ConflictAction, ConflictHandler, GitCli, GitOps, Manifest, MergeOpts, ResolutionMethod,
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
        /// Disable AI conflict resolution (use manual resolution only)
        #[arg(long)]
        no_ai: bool,
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

        if scan.has_markers() {
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
                        if file_scan.has_markers() {
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

        let session_owned = session_name.to_string();
        let action = tokio::task::spawn_blocking(move || {
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
                    session: session_owned.clone(),
                    message: format!("failed to read user input: {e}"),
                })?;
            Ok(match selection {
                0 => ConflictAction::Resolved(ResolutionMethod::Manual),
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

// ── AI Interactive Conflict Handler ──────────────────────────────────

/// Composite handler that attempts AI resolution first, shows a colored
/// unified diff, prompts the user to Accept/Edit/Reject, retries with
/// feedback up to `max_retries`, and falls back to the manual
/// `InteractiveConflictHandler` when AI is exhausted or fails.
struct AiInteractiveConflictHandler<G: GitOps, P: AiProvider + 'static> {
    ai_handler: AiConflictHandler<G, P>,
    provider: Arc<P>,
    config: AiConfig,
    verbose: bool,
}

/// Format a colored unified diff between the original (conflicted) and
/// resolved content for a single file.
fn format_colored_diff(original: &str, resolved: &str, filename: &str) -> String {
    let diff = TextDiff::from_lines(original, resolved);
    let unified = diff
        .unified_diff()
        .context_radius(3)
        .header(
            &format!("a/{filename} (conflicted)"),
            &format!("b/{filename} (ai-resolved)"),
        )
        .to_string();

    let mut colored = String::new();
    for line in unified.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            colored.push_str(&format!("{}\n", console::style(line).green()));
        } else if line.starts_with('-') && !line.starts_with("---") {
            colored.push_str(&format!("{}\n", console::style(line).red()));
        } else if line.starts_with("@@") {
            colored.push_str(&format!("{}\n", console::style(line).cyan()));
        } else {
            colored.push_str(&format!("{line}\n"));
        }
    }
    colored
}

/// Prompt the user to Accept, Edit, or Reject the AI resolution.
/// Returns the selected index (0=Accept, 1=Edit, 2=Reject).
async fn prompt_accept_edit_reject(session_name: &str) -> smelt_core::Result<usize> {
    let session_owned = session_name.to_string();
    tokio::task::spawn_blocking(move || {
        let items = vec![
            "Accept (use AI resolution as-is)",
            "Edit (open files for manual tweaking, then continue)",
            "Reject (provide feedback for retry, or fall back to manual)",
        ];
        dialoguer::Select::new()
            .with_prompt("AI resolution action")
            .items(&items)
            .default(0)
            .interact_on(&console::Term::stderr())
            .map_err(|e| SmeltError::SessionError {
                session: session_owned,
                message: format!("failed to read user input: {e}"),
            })
    })
    .await
    .map_err(|e| SmeltError::SessionError {
        session: session_name.to_string(),
        message: format!("prompt task failed: {e}"),
    })?
}

/// Prompt the user for optional feedback text when rejecting an AI resolution.
async fn prompt_feedback(session_name: &str) -> smelt_core::Result<String> {
    let session_owned = session_name.to_string();
    tokio::task::spawn_blocking(move || {
        dialoguer::Input::<String>::new()
            .with_prompt("Feedback for AI (leave empty to skip)")
            .allow_empty(true)
            .interact_on(&console::Term::stderr())
            .map_err(|e| SmeltError::SessionError {
                session: session_owned,
                message: format!("failed to read feedback: {e}"),
            })
    })
    .await
    .map_err(|e| SmeltError::SessionError {
        session: session_name.to_string(),
        message: format!("feedback prompt task failed: {e}"),
    })?
}

/// Prompt the user to press Enter to continue after editing files.
async fn prompt_continue_after_edit(
    session_name: &str,
    work_dir: &Path,
) -> smelt_core::Result<()> {
    eprintln!(
        "Edit the resolved files in {}, then press Enter to continue.",
        work_dir.display()
    );
    let session_owned = session_name.to_string();
    tokio::task::spawn_blocking(move || {
        dialoguer::Input::<String>::new()
            .with_prompt("Press Enter when done")
            .allow_empty(true)
            .interact_on(&console::Term::stderr())
            .map_err(|e| SmeltError::SessionError {
                session: session_owned,
                message: format!("failed to read input: {e}"),
            })
    })
    .await
    .map_err(|e| SmeltError::SessionError {
        session: session_name.to_string(),
        message: format!("edit prompt task failed: {e}"),
    })?
    .map(|_| ())
}

/// Result of showing the AI diff and prompting the user.
enum AiPromptChoice {
    /// User accepted the AI resolution as-is.
    Accept,
    /// User edited the files after AI resolution.
    Edit,
    /// User rejected the AI resolution.
    Reject,
}

impl<G: GitOps + Send + Sync, P: AiProvider + 'static> AiInteractiveConflictHandler<G, P> {
    /// Show the AI-proposed diff for each file and prompt Accept/Edit/Reject.
    async fn show_diff_and_prompt(
        &self,
        session_name: &str,
        files: &[String],
        original_contents: &[(String, String)],
        work_dir: &Path,
    ) -> smelt_core::Result<AiPromptChoice> {
        // Show diff per file
        eprintln!(
            "\nAI proposed resolution for {} file(s):",
            files.len()
        );
        for (file_path, original) in original_contents {
            let resolved_path = work_dir.join(file_path);
            let resolved = std::fs::read_to_string(&resolved_path).map_err(|e| {
                SmeltError::AiResolution {
                    message: format!("failed to read resolved file '{file_path}': {e}"),
                }
            })?;
            let diff_output = format_colored_diff(original, &resolved, file_path);
            if diff_output.trim().is_empty() {
                eprintln!("  {file_path}: (no changes)");
            } else {
                eprint!("{diff_output}");
            }
        }

        let selection = prompt_accept_edit_reject(session_name).await?;
        match selection {
            0 => Ok(AiPromptChoice::Accept),
            1 => {
                prompt_continue_after_edit(session_name, work_dir).await?;
                Ok(AiPromptChoice::Edit)
            }
            _ => Ok(AiPromptChoice::Reject),
        }
    }

    /// Attempt a retry: rebuild prompt with feedback, call LLM directly,
    /// write resolved content to disk.
    async fn retry_with_feedback(
        &self,
        session_name: &str,
        files: &[String],
        original_contents: &[(String, String)],
        feedback: &str,
        work_dir: &Path,
    ) -> smelt_core::Result<()> {
        let model = self
            .config
            .model
            .as_deref()
            .unwrap_or_else(|| default_model_for_provider(&self.config));

        let system_prompt = build_system_prompt();

        for file in files {
            // Use original conflicted content (with markers) — not the AI-resolved
            // content currently on disk — so the LLM has the real conflict context.
            let conflicted = original_contents
                .iter()
                .find(|(f, _)| f == file)
                .map(|(_, c)| c.as_str())
                .unwrap_or("");

            let original_prompt = build_resolution_prompt(
                file,
                "", // base — not available in retry context
                conflicted,
                "", // theirs — not available in retry context
                session_name,
                None,
                &[],
            );

            let retry_prompt = build_retry_prompt(&original_prompt, feedback);

            let resolved = self
                .provider
                .complete(model, system_prompt, &retry_prompt)
                .await
                .map_err(|e| SmeltError::AiResolution {
                    message: format!("retry failed for '{file}': {e}"),
                })?;

            tokio::fs::write(work_dir.join(file), &resolved)
                .await
                .map_err(|e| SmeltError::AiResolution {
                    message: format!("failed to write retry result for '{file}': {e}"),
                })?;
        }

        Ok(())
    }

    /// Restore original conflicted content to disk.
    fn restore_original_files(
        original_contents: &[(String, String)],
        work_dir: &Path,
    ) {
        for (file_path, content) in original_contents {
            if let Err(e) = std::fs::write(work_dir.join(file_path), content) {
                eprintln!("Warning: failed to restore '{file_path}': {e}");
            }
        }
    }
}

impl<G: GitOps + Send + Sync, P: AiProvider + 'static> ConflictHandler
    for AiInteractiveConflictHandler<G, P>
{
    async fn handle_conflict(
        &self,
        session_name: &str,
        files: &[String],
        scan: &ConflictScan,
        work_dir: &Path,
    ) -> smelt_core::Result<ConflictAction> {
        // If stderr is not a terminal, fall through to manual (which will
        // propagate as an error in non-TTY mode).
        if !console::Term::stderr().is_term() {
            let fallback = InteractiveConflictHandler {
                verbose: self.verbose,
            };
            return fallback
                .handle_conflict(session_name, files, scan, work_dir)
                .await;
        }

        // Save original conflicted content before AI resolution.
        let original_contents: Vec<(String, String)> = files
            .iter()
            .filter_map(|f| {
                match std::fs::read_to_string(work_dir.join(f)) {
                    Ok(content) => Some((f.clone(), content)),
                    Err(e) => {
                        eprintln!("Warning: failed to read conflicted file '{f}': {e}");
                        None
                    }
                }
            })
            .collect();

        if original_contents.len() != files.len() {
            eprintln!(
                "Warning: could not read {}/{} conflicted files — skipping AI resolution",
                files.len() - original_contents.len(),
                files.len()
            );
            let fallback = InteractiveConflictHandler {
                verbose: self.verbose,
            };
            return fallback
                .handle_conflict(session_name, files, scan, work_dir)
                .await;
        }

        // Attempt AI resolution.
        let ai_result = self
            .ai_handler
            .handle_conflict(session_name, files, scan, work_dir)
            .await;

        match ai_result {
            Ok(_action) => {
                // AI succeeded — enter accept/edit/reject loop with retries.
                let mut retries_used: u8 = 0;

                loop {
                    let choice = self
                        .show_diff_and_prompt(
                            session_name,
                            files,
                            &original_contents,
                            work_dir,
                        )
                        .await?;

                    match choice {
                        AiPromptChoice::Accept => {
                            return Ok(ConflictAction::Resolved(
                                ResolutionMethod::AiAssisted,
                            ));
                        }
                        AiPromptChoice::Edit => {
                            return Ok(ConflictAction::Resolved(
                                ResolutionMethod::AiEdited,
                            ));
                        }
                        AiPromptChoice::Reject => {
                            // User rejected — prompt for feedback.
                            let feedback =
                                prompt_feedback(session_name).await?;

                            retries_used += 1;
                            if retries_used <= self.config.max_retries {
                                let feedback_text = if feedback.is_empty() {
                                    "Please try again with a different approach."
                                        .to_string()
                                } else {
                                    feedback
                                };

                                eprintln!(
                                    "Retrying AI resolution ({}/{})...",
                                    retries_used, self.config.max_retries
                                );

                                match self
                                    .retry_with_feedback(
                                        session_name,
                                        files,
                                        &original_contents,
                                        &feedback_text,
                                        work_dir,
                                    )
                                    .await
                                {
                                    Ok(()) => {
                                        // Show diff again and re-prompt.
                                        continue;
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "AI retry failed: {e}"
                                        );
                                        // Fall through to manual.
                                        break;
                                    }
                                }
                            } else {
                                eprintln!(
                                    "AI retries exhausted, falling back to manual resolution..."
                                );
                                break;
                            }
                        }
                    }
                }

                // Retries exhausted or retry failed — fall back to manual.
                Self::restore_original_files(&original_contents, work_dir);
                let fallback = InteractiveConflictHandler {
                    verbose: self.verbose,
                };
                fallback
                    .handle_conflict(session_name, files, scan, work_dir)
                    .await
            }
            Err(e) => {
                // AI resolution failed — graceful fallback.
                eprintln!("AI resolution failed: {e}");
                eprintln!("Falling back to manual resolution...");
                Self::restore_original_files(&original_contents, work_dir);
                let fallback = InteractiveConflictHandler {
                    verbose: self.verbose,
                };
                fallback
                    .handle_conflict(session_name, files, scan, work_dir)
                    .await
            }
        }
    }
}

// ── Enum dispatcher to avoid RPITIT-no-dyn issue ─────────────────────

/// Enum dispatcher for conflict handlers — avoids the RPITIT-no-dyn issue
/// that prevents using `Box<dyn ConflictHandler>`.
enum MergeConflictHandler {
    AiInteractive(Box<AiInteractiveConflictHandler<GitCli, GenAiProvider>>),
    Interactive(InteractiveConflictHandler),
}

impl ConflictHandler for MergeConflictHandler {
    async fn handle_conflict(
        &self,
        session_name: &str,
        files: &[String],
        scan: &ConflictScan,
        work_dir: &Path,
    ) -> smelt_core::Result<ConflictAction> {
        match self {
            Self::AiInteractive(h) => {
                h.handle_conflict(session_name, files, scan, work_dir)
                    .await
            }
            Self::Interactive(h) => {
                h.handle_conflict(session_name, files, scan, work_dir)
                    .await
            }
        }
    }
}

/// Build the appropriate conflict handler based on configuration and flags.
fn build_conflict_handler(
    git: GitCli,
    repo_root: &Path,
    no_ai: bool,
    verbose: bool,
    target_branch: &str,
) -> MergeConflictHandler {
    if no_ai || !console::Term::stderr().is_term() {
        return MergeConflictHandler::Interactive(InteractiveConflictHandler { verbose });
    }

    let smelt_dir = repo_root.join(".smelt");
    let ai_config = match AiConfig::load(&smelt_dir) {
        Some(config) if config.enabled => config,
        Some(_) => {
            eprintln!("AI resolution disabled in config — using manual resolution.");
            return MergeConflictHandler::Interactive(InteractiveConflictHandler { verbose });
        }
        None => {
            // No AI config — use defaults (AI enabled by default).
            AiConfig::default()
        }
    };

    match GenAiProvider::new(&ai_config) {
        Ok(provider) => {
            let provider = Arc::new(provider);
            let ai_handler = AiConflictHandler::new(
                git,
                Arc::clone(&provider),
                ai_config.clone(),
                target_branch.to_string(),
            );
            MergeConflictHandler::AiInteractive(Box::new(AiInteractiveConflictHandler {
                ai_handler,
                provider,
                config: ai_config,
                verbose,
            }))
        }
        Err(e) => {
            eprintln!("Warning: failed to initialize AI provider: {e}");
            eprintln!("Falling back to manual resolution.");
            MergeConflictHandler::Interactive(InteractiveConflictHandler { verbose })
        }
    }
}

// ── Public command functions ─────────────────────────────────────────

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
    no_ai: bool,
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

    // Compute the effective target branch name for handler construction.
    let effective_target = target
        .clone()
        .unwrap_or_else(|| format!("smelt/merge/{}", manifest.manifest.name));

    let handler = build_conflict_handler(
        git.clone(),
        &repo_root,
        no_ai,
        verbose,
        &effective_target,
    );

    let runner = MergeRunner::new(git, repo_root);
    let opts = MergeOpts::new(target, strategy);

    match runner.run(&manifest, opts, &handler).await {
        Ok(report) => {
            // Progress summary to stderr
            for (i, result) in report.sessions_merged.iter().enumerate() {
                let method_note = match result.resolution {
                    ResolutionMethod::AiAssisted => " (AI resolved)",
                    ResolutionMethod::AiEdited => " (AI resolved, user-edited)",
                    ResolutionMethod::Manual => " (manually resolved)",
                    _ => "",
                };
                eprintln!(
                    "[{}/{}] Merged '{}'{}",
                    i + 1,
                    report.sessions_merged.len(),
                    result.session_name,
                    method_note,
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
    let opts = MergeOpts::new(target, strategy);

    match runner.plan(&manifest, opts).await {
        Ok(plan) => {
            if json {
                let output = serde_json::to_string_pretty(&plan)?;
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

    #[test]
    fn test_format_colored_diff_produces_output() {
        let original = "line 1\nline 2\nline 3\n";
        let resolved = "line 1\nmodified line 2\nline 3\n";
        let diff = format_colored_diff(original, resolved, "test.rs");
        assert!(diff.contains("test.rs"));
        // Should contain diff markers (may be styled but raw text is there)
        assert!(!diff.is_empty());
    }

    #[test]
    fn test_format_colored_diff_no_changes() {
        let content = "same content\n";
        let diff = format_colored_diff(content, content, "test.rs");
        // No diff output when content is identical
        assert!(diff.trim().is_empty());
    }
}
