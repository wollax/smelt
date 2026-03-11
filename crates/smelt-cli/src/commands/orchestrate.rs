//! `smelt orchestrate` command handler — orchestration lifecycle management.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Subcommand;
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Table};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use similar::TextDiff;
use tokio_util::sync::CancellationToken;

use smelt_core::ai::{
    AiConfig, AiProvider, GenAiProvider, build_resolution_prompt, build_retry_prompt,
    build_system_prompt,
};
use smelt_core::error::SmeltError;
use smelt_core::merge::conflict::ConflictScan;
use smelt_core::merge::{
    AiConflictHandler, MergeOrderStrategy, default_model_for_provider,
};
use smelt_core::orchestrate::state::compute_manifest_hash;
use smelt_core::orchestrate::types::SessionRunState;
use smelt_core::{
    ConflictAction, ConflictHandler, GitCli, GitOps, Manifest, Orchestrator,
    OrchestrationOpts, OrchestrationReport, ResolutionMethod, RunStateManager,
};

/// Subcommands for `smelt orchestrate`.
#[derive(Subcommand)]
pub enum OrchestrateCommands {
    /// Execute an orchestration plan
    Run {
        /// Path to the manifest TOML file
        manifest: String,
        /// Override merge target branch name
        #[arg(long)]
        target: Option<String>,
        /// Merge ordering strategy (completion-time, file-overlap)
        #[arg(long, value_parser = parse_strategy)]
        strategy: Option<MergeOrderStrategy>,
        /// Show verbose output including session logs
        #[arg(long)]
        verbose: bool,
        /// Disable AI conflict resolution
        #[arg(long)]
        no_ai: bool,
        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Parse a strategy string from the CLI into a `MergeOrderStrategy`.
fn parse_strategy(s: &str) -> Result<MergeOrderStrategy, String> {
    s.parse()
}

// ── Conflict handler types (reuse pattern from merge.rs) ────────────

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
                for file in files {
                    let path = work_dir.join(file);
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let file_scan = smelt_core::merge::scan_conflict_markers(&content);
                        if file_scan.has_markers() {
                            eprintln!("\n  --- {file} ---");
                            let lines: Vec<&str> = content.lines().collect();
                            for hunk in &file_scan.hunks {
                                for ln in hunk.start_line..=hunk.end_line {
                                    if ln <= lines.len() {
                                        let line = lines[ln - 1];
                                        let styled = if line.starts_with("<<<<<<<") {
                                            format!("  {}", console::style(line).red())
                                        } else if line.starts_with("=======") {
                                            format!("  {}", console::style(line).yellow())
                                        } else if line.starts_with(">>>>>>>") {
                                            format!("  {}", console::style(line).green())
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
            eprintln!(
                "\nVerbose: conflict files in worktree at {}",
                work_dir.display()
            );
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

// ── AI Interactive Conflict Handler ─────────────────────────────────

/// Composite handler: AI resolution first, then prompt Accept/Edit/Reject,
/// retry with feedback, fall back to manual.
struct AiInteractiveConflictHandler<G: GitOps, P: AiProvider + 'static> {
    ai_handler: smelt_core::AiConflictHandler<G, P>,
    provider: Arc<P>,
    config: AiConfig,
    verbose: bool,
}

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

enum AiPromptChoice {
    Accept,
    Edit,
    Reject,
}

impl<G: GitOps + Send + Sync, P: AiProvider + 'static> AiInteractiveConflictHandler<G, P> {
    async fn show_diff_and_prompt(
        &self,
        session_name: &str,
        files: &[String],
        original_contents: &[(String, String)],
        work_dir: &Path,
    ) -> smelt_core::Result<AiPromptChoice> {
        eprintln!("\nAI proposed resolution for {} file(s):", files.len());
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
            let conflicted = original_contents
                .iter()
                .find(|(f, _)| f == file)
                .map(|(_, c)| c.as_str())
                .unwrap_or("");

            let original_prompt = build_resolution_prompt(
                file,
                "",
                conflicted,
                "",
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

    fn restore_original_files(original_contents: &[(String, String)], work_dir: &Path) {
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
        if !console::Term::stderr().is_term() {
            let fallback = InteractiveConflictHandler {
                verbose: self.verbose,
            };
            return fallback
                .handle_conflict(session_name, files, scan, work_dir)
                .await;
        }

        let original_contents: Vec<(String, String)> = files
            .iter()
            .filter_map(|f| match std::fs::read_to_string(work_dir.join(f)) {
                Ok(content) => Some((f.clone(), content)),
                Err(e) => {
                    eprintln!("Warning: failed to read conflicted file '{f}': {e}");
                    None
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

        let ai_result = self
            .ai_handler
            .handle_conflict(session_name, files, scan, work_dir)
            .await;

        match ai_result {
            Ok(_action) => {
                let mut retries_used: u8 = 0;

                loop {
                    let choice = self
                        .show_diff_and_prompt(session_name, files, &original_contents, work_dir)
                        .await?;

                    match choice {
                        AiPromptChoice::Accept => {
                            return Ok(ConflictAction::Resolved(ResolutionMethod::AiAssisted));
                        }
                        AiPromptChoice::Edit => {
                            return Ok(ConflictAction::Resolved(ResolutionMethod::AiEdited));
                        }
                        AiPromptChoice::Reject => {
                            let feedback = prompt_feedback(session_name).await?;

                            retries_used += 1;
                            if retries_used <= self.config.max_retries {
                                let feedback_text = if feedback.is_empty() {
                                    "Please try again with a different approach.".to_string()
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
                                    Ok(()) => continue,
                                    Err(e) => {
                                        eprintln!("AI retry failed: {e}");
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

                Self::restore_original_files(&original_contents, work_dir);
                let fallback = InteractiveConflictHandler {
                    verbose: self.verbose,
                };
                fallback
                    .handle_conflict(session_name, files, scan, work_dir)
                    .await
            }
            Err(e) => {
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

// ── Enum dispatcher ─────────────────────────────────────────────────

enum OrchestrateConflictHandler {
    AiInteractive(Box<AiInteractiveConflictHandler<GitCli, GenAiProvider>>),
    Interactive(InteractiveConflictHandler),
}

impl ConflictHandler for OrchestrateConflictHandler {
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

fn build_conflict_handler(
    git: GitCli,
    repo_root: &Path,
    no_ai: bool,
    verbose: bool,
    target_branch: &str,
) -> OrchestrateConflictHandler {
    if no_ai || !console::Term::stderr().is_term() {
        return OrchestrateConflictHandler::Interactive(InteractiveConflictHandler { verbose });
    }

    let smelt_dir = repo_root.join(".smelt");
    let ai_config = match AiConfig::load(&smelt_dir) {
        Some(config) if config.enabled => config,
        Some(_) => {
            eprintln!("AI resolution disabled in config — using manual resolution.");
            return OrchestrateConflictHandler::Interactive(InteractiveConflictHandler { verbose });
        }
        None => AiConfig::default(),
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
            OrchestrateConflictHandler::AiInteractive(Box::new(AiInteractiveConflictHandler {
                ai_handler,
                provider,
                config: ai_config,
                verbose,
            }))
        }
        Err(e) => {
            eprintln!("Warning: failed to initialize AI provider: {e}");
            eprintln!("Falling back to manual resolution.");
            OrchestrateConflictHandler::Interactive(InteractiveConflictHandler { verbose })
        }
    }
}

// ── Dashboard ───────────────────────────────────────────────────────

/// Build a spinner style for a given session state.
fn spinner_style_for(state: &SessionRunState) -> ProgressStyle {
    match state {
        SessionRunState::Pending => ProgressStyle::with_template("  {prefix:.dim} waiting")
            .expect("valid template"),
        SessionRunState::Running => {
            ProgressStyle::with_template("  {spinner:.green} {prefix} running ({elapsed})")
                .expect("valid template")
                .tick_strings(&[
                    "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}",
                    "\u{2826}", "\u{2827}", "\u{2807}", "\u{280f}", "\u{2809}",
                ])
        }
        SessionRunState::Completed { .. } => {
            ProgressStyle::with_template("  {prefix:.green} done ({elapsed})")
                .expect("valid template")
        }
        SessionRunState::Failed { .. } => {
            ProgressStyle::with_template("  {prefix:.red} failed").expect("valid template")
        }
        SessionRunState::Skipped { .. } => {
            ProgressStyle::with_template("  {prefix:.yellow} skipped").expect("valid template")
        }
        SessionRunState::Cancelled => {
            ProgressStyle::with_template("  {prefix:.dim} cancelled").expect("valid template")
        }
    }
}

// ── Summary formatting ──────────────────────────────────────────────

fn format_orchestration_summary(report: &OrchestrationReport) -> String {
    let mut output = String::new();

    output.push_str(&format!(
        "Orchestration: {} (run-id: {})\n\n",
        report.manifest_name, report.run_id
    ));

    // Sessions table
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec!["Session", "Status", "Duration"]);

    // Sort by name for deterministic output
    let mut sessions: Vec<_> = report.session_results.iter().collect();
    sessions.sort_by_key(|(name, _)| (*name).clone());

    for (name, state) in &sessions {
        let (status, duration) = match state {
            SessionRunState::Completed { duration_secs } => {
                ("done".to_string(), format!("{duration_secs:.1}s"))
            }
            SessionRunState::Failed { reason } => {
                ("failed".to_string(), reason.clone())
            }
            SessionRunState::Skipped { reason } => {
                ("skipped".to_string(), reason.clone())
            }
            SessionRunState::Cancelled => ("cancelled".to_string(), "-".to_string()),
            SessionRunState::Pending => ("pending".to_string(), "-".to_string()),
            SessionRunState::Running => ("running".to_string(), "-".to_string()),
        };
        table.add_row(vec![name.to_string(), status, duration]);
    }

    output.push_str("Sessions:\n");
    output.push_str(&table.to_string());
    output.push('\n');

    // Merge summary
    if let Some(ref merge_report) = report.merge_report {
        output.push_str(&format!(
            "\nMerge:\n  Target branch: {}\n  Sessions merged: {}\n  Files changed: {} (+{}, -{})\n",
            merge_report.target_branch,
            merge_report.sessions_merged.len(),
            merge_report.total_files_changed,
            merge_report.total_insertions,
            merge_report.total_deletions,
        ));
    }

    // Summary table (per-session stats)
    if let Some(ref summary) = report.summary {
        output.push('\n');
        output.push_str(&super::summary::format_summary_table(summary));

        if let Some(violations) = super::summary::format_violations(summary) {
            output.push('\n');
            output.push_str(&violations);
        }
    }

    output.push_str(&format!("\nCompleted in {:.1}s\n", report.elapsed_secs));

    output
}

// ── Public command function ─────────────────────────────────────────

/// Execute the `smelt orchestrate run` command.
#[allow(clippy::too_many_arguments)]
pub async fn execute_orchestrate_run(
    git: GitCli,
    repo_root: PathBuf,
    manifest_path: &str,
    target: Option<String>,
    strategy: Option<MergeOrderStrategy>,
    verbose: bool,
    no_ai: bool,
    json: bool,
) -> anyhow::Result<i32> {
    // Load manifest
    let manifest = match Manifest::load(std::path::Path::new(manifest_path)) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error: {e}");
            return Ok(1);
        }
    };

    // Detect agent sessions and print informational message
    let agent_session_count = manifest.sessions.iter().filter(|s| s.script.is_none()).count();
    if agent_session_count > 0 {
        eprintln!(
            "Detected {agent_session_count} agent session(s) — using Claude Code backend"
        );
        // Verify claude is available before proceeding
        if smelt_core::resolve_claude_binary().is_err() {
            eprintln!("Error: 'claude' CLI not found on PATH. Agent sessions require Claude Code to be installed.");
            eprintln!("Install it from: https://docs.anthropic.com/en/docs/claude-code");
            return Ok(1);
        }
    }

    // Read raw manifest content for hash computation
    let manifest_content = match std::fs::read_to_string(manifest_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading manifest: {e}");
            return Ok(1);
        }
    };

    // Set up cancellation
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        eprintln!("\nInterrupted — shutting down gracefully...");
        cancel_clone.cancel();
    });

    // Check for resume
    let smelt_dir = repo_root.join(".smelt");
    let state_manager = RunStateManager::new(&smelt_dir);
    let previous_state = match state_manager.find_incomplete_run(&manifest.manifest.name) {
        Ok(Some(state)) => {
            // Verify manifest hash
            let current_hash = compute_manifest_hash(&manifest_content);
            if current_hash != state.manifest_hash {
                eprintln!(
                    "Warning: manifest has changed since the last run (run-id: {}). Starting fresh.",
                    state.run_id
                );
                None
            } else if console::Term::stderr().is_term() {
                // Prompt for resume
                let prompt_result = tokio::task::spawn_blocking({
                    let run_id = state.run_id.clone();
                    move || {
                        dialoguer::Confirm::new()
                            .with_prompt(format!(
                                "Previous incomplete run found ({run_id}). Resume?"
                            ))
                            .default(true)
                            .interact_on(&console::Term::stderr())
                    }
                })
                .await;

                match prompt_result {
                    Ok(Ok(true)) => {
                        eprintln!("Resuming run '{}'...", state.run_id);
                        Some(state)
                    }
                    _ => None,
                }
            } else {
                // Non-TTY: skip resume
                None
            }
        }
        Ok(None) => None,
        Err(e) => {
            eprintln!("Warning: failed to check for resume: {e}");
            None
        }
    };

    // Build conflict handler
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

    // Build orchestration opts
    let opts = OrchestrationOpts {
        target_branch: target,
        strategy,
        verbose,
        no_ai,
        json,
    };

    // Set up dashboard
    let is_tty = console::Term::stderr().is_term();
    let use_dashboard = is_tty && !json;

    let session_names: Vec<String> = manifest.sessions.iter().map(|s| s.name.clone()).collect();

    let (mp, bars): (Option<MultiProgress>, HashMap<String, ProgressBar>) = if use_dashboard {
        let mp = MultiProgress::new();
        let mut bars = HashMap::new();

        for name in &session_names {
            let pb = mp.add(ProgressBar::new_spinner());
            pb.set_prefix(name.clone());
            pb.set_style(spinner_style_for(&SessionRunState::Pending));
            pb.enable_steady_tick(std::time::Duration::from_millis(100));
            bars.insert(name.clone(), pb);
        }

        (Some(mp), bars)
    } else {
        (None, HashMap::new())
    };

    // Status callback
    let on_status = move |name: &str, state: &SessionRunState| {
        if let Some(pb) = bars.get(name) {
            pb.set_style(spinner_style_for(state));
            if state.is_terminal() {
                pb.finish();
            }
        } else if !json {
            // Line-by-line fallback
            let status_str = match state {
                SessionRunState::Pending => "pending",
                SessionRunState::Running => "running",
                SessionRunState::Completed { .. } => "done",
                SessionRunState::Failed { .. } => "failed",
                SessionRunState::Skipped { .. } => "skipped",
                SessionRunState::Cancelled => "cancelled",
            };
            eprintln!("[{name}] {status_str}");
        }
    };

    // Execute
    let orchestrator = Orchestrator::new(git, repo_root);

    let result = if let Some(prev_state) = previous_state {
        orchestrator
            .resume(
                &manifest,
                &manifest_content,
                prev_state,
                &opts,
                &handler,
                cancel,
                on_status,
            )
            .await
    } else {
        orchestrator
            .run(
                &manifest,
                &manifest_content,
                &opts,
                &handler,
                cancel,
                on_status,
            )
            .await
    };

    // Clear multiProgress
    drop(mp);

    match result {
        Ok(report) => {
            let has_failures = report.session_results.values().any(|s| {
                matches!(
                    s,
                    SessionRunState::Failed { .. }
                        | SessionRunState::Skipped { .. }
                        | SessionRunState::Cancelled
                )
            });

            if json {
                let output = serde_json::to_string_pretty(&report)?;
                println!("{output}");
            } else {
                print!("{}", format_orchestration_summary(&report));
            }

            if has_failures { Ok(1) } else { Ok(0) }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            Ok(1)
        }
    }
}
