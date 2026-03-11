//! Scope checking logic for session file isolation.
//!
//! Scope checking is opt-in: sessions without a `file_scope` have no violations.
//! Files matching `shared_files` globs are always considered in-scope for all sessions.

use globset::{GlobSet, GlobSetBuilder};
use tracing::warn;

use crate::error::SmeltError;
use crate::summary::types::ScopeViolation;

/// Check which files are out of scope for a session.
///
/// Returns an empty `Vec` if `file_scope` is `None` — scope checking is opt-in.
/// A file is in-scope if it matches any `file_scope` glob OR any `shared_files` glob.
/// All other files are violations.
pub fn check_scope(
    session_name: &str,
    file_scope: Option<&[String]>,
    shared_files: &[String],
    changed_files: &[String],
) -> Vec<ScopeViolation> {
    let Some(scope_patterns) = file_scope else {
        return Vec::new();
    };

    let matcher = match build_scope_matcher(scope_patterns, shared_files) {
        Ok(m) => m,
        Err(e) => {
            // Globs should have been validated at manifest parse time.
            // If they somehow fail here, treat everything as a violation.
            warn!(
                "Failed to build scope matcher for session '{session_name}': {e}; \
                 treating all changed files as violations"
            );
            return changed_files
                .iter()
                .map(|f| ScopeViolation {
                    session_name: session_name.to_string(),
                    file_path: f.clone(),
                    file_scope: scope_patterns.to_vec(),
                })
                .collect();
        }
    };

    changed_files
        .iter()
        .filter(|f| !matcher.is_match(f.as_str()))
        .map(|f| ScopeViolation {
            session_name: session_name.to_string(),
            file_path: f.clone(),
            file_scope: scope_patterns.to_vec(),
        })
        .collect()
}

/// Build a `GlobSet` from `file_scope` and `shared_files` patterns.
///
/// Callers should have already validated globs via `Manifest::validate()`.
/// This function still returns a `Result` for robustness.
fn build_scope_matcher(
    file_scope: &[String],
    shared_files: &[String],
) -> crate::Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in file_scope.iter().chain(shared_files.iter()) {
        let glob = globset::Glob::new(pattern).map_err(|e| SmeltError::Orchestration {
            message: format!("invalid glob pattern '{pattern}': {e}"),
        })?;
        builder.add(glob);
    }
    builder.build().map_err(|e| SmeltError::Orchestration {
        message: format!("failed to build GlobSet: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_file_scope_means_no_violations() {
        let violations = check_scope(
            "session-a",
            None,
            &[],
            &["src/main.rs".to_string(), "README.md".to_string()],
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn all_files_in_scope() {
        let violations = check_scope(
            "session-a",
            Some(&["src/**".to_string()]),
            &[],
            &["src/main.rs".to_string(), "src/lib.rs".to_string()],
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn out_of_scope_files_detected() {
        let violations = check_scope(
            "session-a",
            Some(&["src/auth/**".to_string()]),
            &[],
            &[
                "src/auth/login.rs".to_string(),
                "src/db/schema.rs".to_string(),
            ],
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].file_path, "src/db/schema.rs");
        assert_eq!(violations[0].session_name, "session-a");
    }

    #[test]
    fn shared_files_override_scope() {
        let violations = check_scope(
            "session-a",
            Some(&["src/auth/**".to_string()]),
            &["Cargo.toml".to_string(), "Cargo.lock".to_string()],
            &[
                "src/auth/login.rs".to_string(),
                "Cargo.toml".to_string(),
            ],
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn multiple_file_scope_patterns() {
        let violations = check_scope(
            "session-a",
            Some(&["src/auth/**".to_string(), "src/lib.rs".to_string()]),
            &[],
            &[
                "src/auth/login.rs".to_string(),
                "src/lib.rs".to_string(),
                "src/db/mod.rs".to_string(),
            ],
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].file_path, "src/db/mod.rs");
    }

    #[test]
    fn empty_file_scope_means_all_violations() {
        let violations = check_scope(
            "session-a",
            Some(&[]),
            &[],
            &["a.txt".to_string()],
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].file_path, "a.txt");
    }

    #[test]
    fn violation_captures_session_file_scope() {
        let scope = vec!["src/auth/**".to_string()];
        let violations = check_scope(
            "session-b",
            Some(&scope),
            &[],
            &["src/db/schema.rs".to_string()],
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].session_name, "session-b");
        assert_eq!(violations[0].file_path, "src/db/schema.rs");
        assert_eq!(violations[0].file_scope, vec!["src/auth/**".to_string()]);
    }
}
