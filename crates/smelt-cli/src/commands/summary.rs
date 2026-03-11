//! `smelt summary` command handler — display session summaries and scope violations.

use std::path::PathBuf;

use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Table};

use smelt_core::summary::SummaryReport;
use smelt_core::{Manifest, RunStateManager};

/// Arguments for the `smelt summary` command.
#[derive(clap::Args)]
pub struct SummaryArgs {
    /// Path to the manifest TOML file
    pub manifest: String,

    /// Show a specific run (defaults to latest completed run)
    #[arg(long)]
    pub run_id: Option<String>,

    /// Output the summary report as JSON
    #[arg(long)]
    pub json: bool,

    /// Show per-session file lists with line counts
    #[arg(long)]
    pub verbose: bool,
}

/// Execute the `smelt summary` command.
pub async fn execute_summary(
    repo_root: PathBuf,
    args: SummaryArgs,
) -> anyhow::Result<i32> {
    // Load manifest to derive manifest name
    let manifest = match Manifest::load(std::path::Path::new(&args.manifest)) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error: {e}");
            return Ok(1);
        }
    };

    let smelt_dir = repo_root.join(".smelt");
    let state_manager = RunStateManager::new(&smelt_dir);

    // Resolve run_id
    let run_id = if let Some(id) = args.run_id {
        id
    } else {
        match state_manager.find_latest_completed_run(&manifest.manifest.name) {
            Ok(Some(id)) => id,
            Ok(None) => {
                eprintln!("No completed runs found for manifest '{}'.", manifest.manifest.name);
                return Ok(1);
            }
            Err(e) => {
                eprintln!("Error finding completed runs: {e}");
                return Ok(1);
            }
        }
    };

    // Load summary
    let report = match state_manager.load_summary(&run_id) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error loading summary for run '{run_id}': {e}");
            return Ok(1);
        }
    };

    if args.json {
        let output = serde_json::to_string_pretty(&report)?;
        println!("{output}");
        return Ok(0);
    }

    // Human-readable output
    print!("{}", format_summary_table(&report));

    if let Some(violations) = format_violations(&report) {
        print!("\n{violations}");
    }

    if args.verbose {
        print!("\n{}", format_summary_verbose(&report));
    }

    Ok(0)
}

/// Format a compact summary table with Session | Files | +Lines | -Lines columns.
pub fn format_summary_table(report: &SummaryReport) -> String {
    let mut output = String::new();

    output.push_str("Summary:\n");

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_header(vec!["Session", "Files", "+Lines", "-Lines"]);

    for session in &report.sessions {
        table.add_row(vec![
            session.session_name.clone(),
            session.files_changed().to_string(),
            format!("+{}", session.total_insertions),
            format!("-{}", session.total_deletions),
        ]);
    }

    // Totals row
    table.add_row(vec![
        "Total".to_string(),
        report.totals.files_changed.to_string(),
        format!("+{}", report.totals.insertions),
        format!("-{}", report.totals.deletions),
    ]);

    output.push_str(&table.to_string());
    output.push('\n');

    output
}

/// Format scope violations section. Returns `None` if no violations exist.
pub fn format_violations(report: &SummaryReport) -> Option<String> {
    if !report.has_violations() {
        return None;
    }

    let mut output = String::new();
    output.push_str(&format!(
        "Scope violations: {} file(s) outside declared scope\n",
        report.totals.violations
    ));

    for session in &report.sessions {
        if session.violations.is_empty() {
            continue;
        }

        output.push_str(&format!(
            "\n  {} ({} file(s) outside scope):\n",
            session.session_name,
            session.violations.len()
        ));

        for violation in &session.violations {
            output.push_str(&format!("    - {}\n", violation.file_path));
        }

        if let Some(first) = session.violations.first()
            && !first.file_scope.is_empty()
        {
            output.push_str(&format!(
                "    file_scope: [{}]\n",
                first
                    .file_scope
                    .iter()
                    .map(|s| format!("\"{s}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    Some(output)
}

/// Format verbose per-session file lists with line counts and commit messages.
fn format_summary_verbose(report: &SummaryReport) -> String {
    let mut output = String::new();

    output.push_str("Details:\n");

    for session in &report.sessions {
        output.push_str(&format!("\n  {}:\n", session.session_name));

        for file in &session.files {
            output.push_str(&format!(
                "    {} (+{}, -{})\n",
                file.path, file.insertions, file.deletions
            ));
        }

        if !session.commit_messages.is_empty() {
            output.push_str("    Commits:\n");
            for msg in &session.commit_messages {
                output.push_str(&format!("      - {msg}\n"));
            }
        }
    }

    output
}
