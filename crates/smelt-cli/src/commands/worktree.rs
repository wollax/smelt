//! `smelt worktree` (alias: `smelt wt`) command handlers.

use std::path::PathBuf;

use clap::Subcommand;
use smelt_core::{CreateWorktreeOpts, GitCli, SmeltError, WorktreeManager};

/// Resolve a path to an absolute display string, collapsing `..` segments.
fn display_path(path: &std::path::Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

/// Worktree subcommands for managing agent session worktrees.
#[derive(Subcommand)]
pub enum WorktreeCommands {
    /// Create a new worktree for an agent session
    Create {
        /// Session name (used for branch and directory naming)
        name: String,

        /// Base branch or commit (defaults to HEAD)
        #[arg(long, default_value = "HEAD")]
        base: String,

        /// Custom worktree directory name
        #[arg(long)]
        dir_name: Option<String>,

        /// Task description for the session
        #[arg(long)]
        task: Option<String>,
    },

    /// List all tracked worktrees
    List {
        /// Show detailed output including base ref and timestamps
        #[arg(short, long)]
        verbose: bool,
    },

    /// Remove a worktree and its branch
    Remove {
        /// Session name
        name: String,

        /// Force removal even with unmerged changes or dirty worktree
        #[arg(short, long)]
        force: bool,

        /// Skip confirmation prompts
        #[arg(short, long)]
        yes: bool,
    },

    /// Clean up orphaned worktrees
    Prune {
        /// Skip confirmation prompts
        #[arg(short, long)]
        yes: bool,
    },
}

/// Execute the `worktree create` subcommand.
pub async fn execute_create(
    git: GitCli,
    repo_root: PathBuf,
    name: &str,
    base: &str,
    dir_name: Option<String>,
    task: Option<String>,
) -> anyhow::Result<i32> {
    let manager = WorktreeManager::new(git, repo_root);

    let opts = CreateWorktreeOpts {
        session_name: name.to_string(),
        base: base.to_string(),
        dir_name,
        task_description: task,
        file_scope: None,
    };

    match manager.create(opts).await {
        Ok(info) => {
            let abs_path = std::path::absolute(&info.worktree_path)
                .unwrap_or_else(|_| info.worktree_path.clone());
            println!("Created worktree '{}'", info.session_name);
            println!("  Branch: {}", info.branch_name);
            println!("  Path:   {}", abs_path.display());
            Ok(0)
        }
        Err(SmeltError::NotInitialized) => {
            eprintln!("Error: not a Smelt project (run `smelt init` first)");
            Ok(1)
        }
        Err(SmeltError::WorktreeExists { name }) => {
            eprintln!("Error: worktree '{name}' already exists");
            Ok(1)
        }
        Err(SmeltError::BranchExists { branch }) => {
            eprintln!("Error: branch '{branch}' already exists");
            Ok(1)
        }
        Err(e) => Err(e.into()),
    }
}

/// Execute the `worktree list` subcommand.
pub async fn execute_list(
    git: GitCli,
    repo_root: PathBuf,
    verbose: bool,
) -> anyhow::Result<i32> {
    let manager = WorktreeManager::new(git, repo_root);

    let worktrees = manager.list().await?;

    if worktrees.is_empty() {
        println!("No worktrees tracked.");
        return Ok(0);
    }

    if verbose {
        // Verbose: NAME | BRANCH | STATUS | BASE | CREATED | PATH
        println!(
            "{:<20} {:<30} {:<12} {:<10} {:<22} PATH",
            "NAME", "BRANCH", "STATUS", "BASE", "CREATED"
        );
        println!("{}", "-".repeat(110));
        for wt in &worktrees {
            let status = format!("{:?}", wt.status).to_lowercase();
            let created = wt.created_at.format("%Y-%m-%d %H:%M:%S").to_string();
            println!(
                "{:<20} {:<30} {:<12} {:<10} {:<22} {}",
                wt.session_name,
                wt.branch_name,
                status,
                wt.base_ref,
                created,
                display_path(&wt.worktree_path),
            );
        }
    } else {
        // Compact: NAME | BRANCH | STATUS | PATH
        println!(
            "{:<20} {:<30} {:<12} PATH",
            "NAME", "BRANCH", "STATUS"
        );
        println!("{}", "-".repeat(80));
        for wt in &worktrees {
            let status = format!("{:?}", wt.status).to_lowercase();
            println!(
                "{:<20} {:<30} {:<12} {}",
                wt.session_name,
                wt.branch_name,
                status,
                display_path(&wt.worktree_path),
            );
        }
    }

    Ok(0)
}

/// Execute the `worktree remove` subcommand.
pub async fn execute_remove(
    git: GitCli,
    repo_root: PathBuf,
    name: &str,
    force: bool,
    yes: bool,
) -> anyhow::Result<i32> {
    let manager = WorktreeManager::new(git, repo_root);

    // First attempt with the requested force level
    match manager.remove(name, force).await {
        Ok(result) => {
            print_remove_result(&result);
            Ok(0)
        }
        Err(SmeltError::WorktreeNotFound { name }) => {
            eprintln!("Error: worktree '{name}' not found");
            Ok(1)
        }
        Err(SmeltError::WorktreeDirty { name }) => {
            let should_force = if yes {
                // --yes flag: auto-confirm
                true
            } else {
                // Prompt for confirmation if interactive
                dialoguer::Confirm::new()
                    .with_prompt(format!(
                        "Worktree '{name}' has uncommitted changes. Remove anyway?"
                    ))
                    .default(false)
                    .interact()
                    .unwrap_or(false)
            };

            if should_force {
                let result = manager.remove(&name, true).await?;
                print_remove_result(&result);
                Ok(0)
            } else {
                eprintln!(
                    "Error: worktree '{name}' has uncommitted changes (use --force to override)"
                );
                Ok(1)
            }
        }
        Err(SmeltError::BranchUnmerged { branch }) => {
            eprintln!(
                "Warning: branch '{branch}' has unmerged commits. Use --force to delete anyway."
            );
            Ok(1)
        }
        Err(e) => Err(e.into()),
    }
}

/// Print details of a successful worktree removal.
fn print_remove_result(result: &smelt_core::RemoveResult) {
    println!("Removed worktree '{}'", result.session_name);
    if result.worktree_removed {
        println!("  Worktree directory removed");
    }
    if result.branch_deleted {
        println!("  Branch deleted");
    }
    if result.state_file_removed {
        println!("  State file removed");
    }
}

/// Execute the `worktree prune` subcommand.
pub async fn execute_prune(
    git: GitCli,
    repo_root: PathBuf,
    yes: bool,
) -> anyhow::Result<i32> {
    let manager = WorktreeManager::new(git, repo_root);

    let orphans = manager.detect_orphans().await?;

    if orphans.is_empty() {
        println!("No orphaned worktrees found.");
        return Ok(0);
    }

    println!("Found {} orphaned worktree(s):", orphans.len());
    for (name, _state) in &orphans {
        println!("  - {name}");
    }

    if !yes {
        let confirmed = dialoguer::Confirm::new()
            .with_prompt("Remove all orphaned worktrees?")
            .default(false)
            .interact()
            .unwrap_or(false);

        if !confirmed {
            println!("Aborted.");
            return Ok(0);
        }
    }

    let pruned = manager.prune().await?;
    for name in &pruned {
        println!("Pruned: {name}");
    }

    println!("Pruned {} orphaned worktree(s).", pruned.len());
    Ok(0)
}
