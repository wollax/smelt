//! `smelt init` command handler.

use std::path::Path;

use smelt_core::SmeltError;

/// Execute the `init` subcommand: create `.smelt/config.toml` at the repository root.
///
/// Returns exit code `0` on success, `1` if already initialized.
pub fn execute(repo_root: &Path) -> anyhow::Result<i32> {
    match smelt_core::init_project(repo_root) {
        Ok(smelt_dir) => {
            println!("Initialized Smelt project in {}", smelt_dir.display());
            println!("Run `git add .smelt/` to track Smelt configuration.");
            Ok(0)
        }
        Err(SmeltError::AlreadyInitialized { path }) => {
            eprintln!(
                "Error: .smelt/ already exists in {}. Already initialized.",
                path.display()
            );
            Ok(1)
        }
        Err(e) => Err(e.into()),
    }
}
