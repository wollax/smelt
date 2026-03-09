//! Project initialization — `.smelt/` directory creation.

use std::path::{Path, PathBuf};

use crate::error::{Result, SmeltError};

/// Default content for a freshly created `.smelt/config.toml`.
const DEFAULT_CONFIG: &str = "\
# Smelt project configuration\n\
\n\
# Smelt format version (for future migration support)\n\
version = 1\n";

/// Initialize a new Smelt project by creating `.smelt/config.toml` at the
/// repository root.
///
/// Returns the path to the created `.smelt/` directory.
///
/// # Errors
///
/// - [`SmeltError::AlreadyInitialized`] if `.smelt/` already exists.
/// - [`SmeltError::Io`] if directory creation or file writing fails.
///   On write failure the partially-created `.smelt/` directory is removed
///   (best-effort cleanup).
pub fn init_project(repo_root: &Path) -> Result<PathBuf> {
    let smelt_dir = repo_root.join(".smelt");

    if smelt_dir.exists() {
        return Err(SmeltError::AlreadyInitialized { path: smelt_dir });
    }

    std::fs::create_dir(&smelt_dir)
        .map_err(|e| SmeltError::io("creating .smelt directory", &smelt_dir, e))?;

    let config_path = smelt_dir.join("config.toml");
    if let Err(e) = std::fs::write(&config_path, DEFAULT_CONFIG) {
        // Best-effort cleanup — ignore errors during removal.
        let _ = std::fs::remove_dir_all(&smelt_dir);
        return Err(SmeltError::io(
            "writing .smelt/config.toml",
            &config_path,
            e,
        ));
    }

    Ok(smelt_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_creates_smelt_dir() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let smelt_dir = init_project(tmp.path()).expect("init_project should succeed");

        assert!(smelt_dir.is_dir(), ".smelt/ should be a directory");

        let config = std::fs::read_to_string(smelt_dir.join("config.toml"))
            .expect("config.toml should exist");
        assert!(
            config.contains("version = 1"),
            "config.toml should contain version = 1, got: {config}",
        );
    }

    #[test]
    fn test_init_already_initialized() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        init_project(tmp.path()).expect("first init should succeed");

        let err = init_project(tmp.path()).expect_err("second init should fail");
        assert!(
            matches!(err, SmeltError::AlreadyInitialized { .. }),
            "expected AlreadyInitialized, got: {err}",
        );
    }
}
