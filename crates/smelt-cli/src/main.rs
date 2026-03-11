//! Smelt CLI — multi-agent orchestration for git.

mod commands;

use clap::{CommandFactory, Parser, Subcommand};
use smelt_core::{GitCli, GitOps};

#[derive(Parser)]
#[command(
    name = "smelt",
    about = "Multi-agent orchestration for git",
    propagate_version = true,
    version
)]
struct Cli {
    /// Disable colored output
    #[arg(long)]
    no_color: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new Smelt project in the current repository
    Init,

    /// Manage worktrees for agent sessions
    #[command(visible_alias = "wt")]
    Worktree {
        #[command(subcommand)]
        command: commands::worktree::WorktreeCommands,
    },

    /// Manage and run agent sessions
    Session {
        #[command(subcommand)]
        command: commands::session::SessionCommands,
    },

    /// Merge session outputs into a single branch
    Merge {
        #[command(subcommand)]
        command: commands::merge::MergeCommands,
    },

    /// Orchestrate multi-session execution with dependency management
    #[command(visible_alias = "orch")]
    Orchestrate {
        #[command(subcommand)]
        command: commands::orchestrate::OrchestrateCommands,
    },

    /// Show summary of a completed orchestration run
    Summary(commands::summary::SummaryArgs),
}

async fn run() -> anyhow::Result<i32> {
    let cli = Cli::try_parse().unwrap_or_else(|e| e.exit());

    if cli.no_color {
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }

    // Initialize tracing subscriber
    let env_filter = tracing_subscriber::EnvFilter::try_from_env("SMELT_LOG")
        .or_else(|_| tracing_subscriber::EnvFilter::try_from_env("RUST_LOG"))
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .init();

    // Preflight: locate git binary and repo root
    let (git_binary, repo_root) = match smelt_core::preflight() {
        Ok(result) => result,
        Err(e) => {
            eprintln!("Error: {e}");
            return Ok(1);
        }
    };

    match cli.command {
        Some(Commands::Init) => commands::init::execute(&repo_root),
        Some(Commands::Worktree { command }) => {
            let git = GitCli::new(git_binary, repo_root.clone());
            match command {
                commands::worktree::WorktreeCommands::Create {
                    name,
                    base,
                    dir_name,
                    task,
                } => {
                    commands::worktree::execute_create(git, repo_root, &name, &base, dir_name, task)
                        .await
                }
                commands::worktree::WorktreeCommands::List { verbose } => {
                    commands::worktree::execute_list(git, repo_root, verbose).await
                }
                commands::worktree::WorktreeCommands::Remove { name, force, yes } => {
                    commands::worktree::execute_remove(git, repo_root, &name, force, yes)
                        .await
                }
                commands::worktree::WorktreeCommands::Prune { yes } => {
                    commands::worktree::execute_prune(git, repo_root, yes).await
                }
            }
        }
        Some(Commands::Session { command }) => {
            let git = GitCli::new(git_binary, repo_root.clone());
            match command {
                commands::session::SessionCommands::Run { manifest } => {
                    commands::session::execute_run(git, repo_root, &manifest).await
                }
            }
        }
        Some(Commands::Merge { command }) => {
            let git = GitCli::new(git_binary, repo_root.clone());
            match command {
                commands::merge::MergeCommands::Run {
                    manifest,
                    target,
                    strategy,
                    verbose,
                    no_ai,
                } => {
                    commands::merge::execute_merge_run(
                        git, repo_root, &manifest, target, strategy, verbose, no_ai,
                    )
                    .await
                }
                commands::merge::MergeCommands::Plan {
                    manifest,
                    target,
                    strategy,
                    json,
                } => {
                    commands::merge::execute_merge_plan(
                        git, repo_root, &manifest, target, strategy, json,
                    )
                    .await
                }
            }
        }
        Some(Commands::Orchestrate { command }) => {
            let git = GitCli::new(git_binary, repo_root.clone());
            match command {
                commands::orchestrate::OrchestrateCommands::Run {
                    manifest,
                    target,
                    strategy,
                    verbose,
                    no_ai,
                    json,
                } => {
                    commands::orchestrate::execute_orchestrate_run(
                        git, repo_root, &manifest, target, strategy, verbose, no_ai, json,
                    )
                    .await
                }
            }
        }
        Some(Commands::Summary(args)) => {
            commands::summary::execute_summary(repo_root, args).await
        }
        None => {
            let smelt_dir = repo_root.join(".smelt");
            if smelt_dir.exists() {
                // Inside a Smelt project — show basic status
                let git = GitCli::new(git_binary, repo_root.clone());
                let branch = git
                    .current_branch()
                    .await
                    .unwrap_or_else(|_| "unknown".into());
                let project_name = repo_root
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown".into());
                println!("Smelt project: {project_name}");
                println!("Branch: {branch}");
                Ok(0)
            } else {
                eprintln!("Not a Smelt project. Run `smelt init` to get started.");
                // Print help to stderr as guidance
                let mut cmd = Cli::command();
                cmd.print_help()?;
                eprintln!();
                Ok(1)
            }
        }
    }
}

#[tokio::main]
async fn main() {
    match run().await {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("Error: {e:#}");
            std::process::exit(1);
        }
    }
}
