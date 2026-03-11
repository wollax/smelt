//! Scope checking logic for session file isolation.

use crate::summary::types::ScopeViolation;

/// Check which files are out of scope for a session.
///
/// Returns an empty `Vec` if `file_scope` is `None` — scope checking is opt-in.
/// A file is in-scope if it matches any `file_scope` glob OR any `shared_files` glob.
/// All other files are violations.
pub fn check_scope(
    _session_name: &str,
    _file_scope: Option<&[String]>,
    _shared_files: &[String],
    _changed_files: &[String],
) -> Vec<ScopeViolation> {
    // Implementation in Task 2
    Vec::new()
}
