//! Session manifest types, TOML parsing, and validation.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Result, SmeltError};
use crate::orchestrate::types::FailurePolicy;

/// Top-level session manifest, parsed from a TOML file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub manifest: ManifestMeta,
    #[serde(rename = "session")]
    pub sessions: Vec<SessionDef>,
}

/// Manifest-level metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestMeta {
    pub name: String,
    #[serde(default = "default_base_ref")]
    pub base_ref: String,
    /// Merge ordering strategy (optional; CLI flag overrides this).
    pub merge_strategy: Option<crate::merge::types::MergeOrderStrategy>,
    /// Whether sessions run in parallel by default (default: true).
    /// Sessions without explicit `depends_on` run concurrently when true,
    /// or sequentially in manifest order when false.
    #[serde(default = "default_parallel")]
    pub parallel_by_default: bool,
    /// Failure policy for orchestration — governs behavior when a session fails.
    pub on_failure: Option<FailurePolicy>,
}

fn default_base_ref() -> String {
    "HEAD".to_string()
}

fn default_parallel() -> bool {
    true
}

/// Definition of a single session within a manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDef {
    pub name: String,
    /// Inline task description.
    pub task: Option<String>,
    /// Path to external task description file.
    pub task_file: Option<String>,
    /// Glob patterns for file scope.
    pub file_scope: Option<Vec<String>>,
    /// Base ref override for this session.
    pub base_ref: Option<String>,
    /// Timeout in seconds.
    pub timeout_secs: Option<u64>,
    /// Environment variable overrides.
    pub env: Option<HashMap<String, String>>,
    /// Sessions that must complete before this session starts.
    pub depends_on: Option<Vec<String>>,
    /// Script definition (required for scripted backend).
    pub script: Option<ScriptDef>,
}

/// Declarative script definition for scripted sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptDef {
    #[serde(default = "default_backend")]
    pub backend: String,
    /// Exit after N steps (simulates early termination).
    pub exit_after: Option<usize>,
    /// Failure mode to simulate.
    pub simulate_failure: Option<FailureMode>,
    pub steps: Vec<ScriptStep>,
}

fn default_backend() -> String {
    "scripted".to_string()
}

/// Failure simulation modes for scripted sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FailureMode {
    Crash,
    Hang,
    Partial,
}

/// A single step in a scripted session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum ScriptStep {
    Commit {
        message: String,
        files: Vec<FileChange>,
    },
}

/// A file change within a script step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    /// Inline content.
    pub content: Option<String>,
    /// Path to file containing content.
    pub content_file: Option<String>,
}

impl Manifest {
    /// Load and validate a manifest from a TOML file.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| SmeltError::io("reading manifest", path, e))?;
        let manifest: Manifest = toml::from_str(&content)
            .map_err(|e| SmeltError::ManifestParse(format!("{}: {e}", path.display())))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Parse and validate a manifest from a TOML string.
    pub fn parse(s: &str) -> Result<Self> {
        let manifest: Manifest =
            toml::from_str(s).map_err(|e| SmeltError::ManifestParse(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate manifest invariants.
    fn validate(&self) -> Result<()> {
        if self.sessions.is_empty() {
            return Err(SmeltError::ManifestParse(
                "manifest must define at least one session".to_string(),
            ));
        }

        let mut names = HashSet::new();
        for session in &self.sessions {
            if !names.insert(&session.name) {
                return Err(SmeltError::ManifestParse(format!(
                    "duplicate session name: '{}'",
                    session.name
                )));
            }

            if session.task.is_none() && session.task_file.is_none() {
                return Err(SmeltError::ManifestParse(format!(
                    "session '{}' must have either 'task' or 'task_file'",
                    session.name
                )));
            }

            if session.task.is_some() && session.task_file.is_some() {
                return Err(SmeltError::ManifestParse(format!(
                    "session '{}' cannot have both 'task' and 'task_file'",
                    session.name
                )));
            }

            if let Some(ref script) = session.script
                && script.steps.is_empty()
            {
                return Err(SmeltError::ManifestParse(format!(
                    "session '{}' script must have at least one step",
                    session.name
                )));
            }

            // Validate file_scope globs
            if let Some(ref scopes) = session.file_scope {
                for pattern in scopes {
                    if let Err(e) = globset::Glob::new(pattern) {
                        return Err(SmeltError::ManifestParse(format!(
                            "session '{}' has invalid glob pattern '{}': {e}",
                            session.name, pattern
                        )));
                    }
                }
            }

            // Validate depends_on references
            if let Some(ref deps) = session.depends_on {
                for dep in deps {
                    // Self-dependency check
                    if dep == &session.name {
                        return Err(SmeltError::ManifestParse(format!(
                            "session '{}' cannot depend on itself",
                            session.name
                        )));
                    }
                }
            }
        }

        // Second pass: validate depends_on references exist and check for cycles
        let name_set: HashSet<&str> = self.sessions.iter().map(|s| s.name.as_str()).collect();
        for session in &self.sessions {
            if let Some(ref deps) = session.depends_on {
                for dep in deps {
                    if !name_set.contains(dep.as_str()) {
                        return Err(SmeltError::ManifestParse(format!(
                            "session '{}' depends on unknown session '{dep}'",
                            session.name
                        )));
                    }
                }
            }
        }

        // Cycle detection using petgraph
        self.validate_no_cycles()?;

        Ok(())
    }

    /// Build a dependency graph and check for cycles.
    fn validate_no_cycles(&self) -> Result<()> {
        use petgraph::algo::is_cyclic_directed;
        use petgraph::graph::DiGraph;

        let mut graph = DiGraph::<&str, ()>::new();
        let mut name_to_idx = HashMap::new();

        for session in &self.sessions {
            let idx = graph.add_node(session.name.as_str());
            name_to_idx.insert(session.name.as_str(), idx);
        }

        // Add explicit dependency edges
        for session in &self.sessions {
            if let Some(ref deps) = session.depends_on {
                let to = name_to_idx[session.name.as_str()];
                for dep in deps {
                    let from = name_to_idx[dep.as_str()];
                    graph.add_edge(from, to, ());
                }
            }
        }

        // Add implicit sequential edges when parallel_by_default=false
        if !self.manifest.parallel_by_default {
            let no_deps: Vec<&str> = self
                .sessions
                .iter()
                .filter(|s| s.depends_on.is_none())
                .map(|s| s.name.as_str())
                .collect();
            for pair in no_deps.windows(2) {
                let from = name_to_idx[pair[0]];
                let to = name_to_idx[pair[1]];
                graph.add_edge(from, to, ());
            }
        }

        if is_cyclic_directed(&graph) {
            // Find the cycle participants for a useful error message
            let cycle_names: Vec<&str> = graph.node_indices().map(|n| graph[n]).collect();
            return Err(SmeltError::DependencyCycle {
                details: format!(
                    "sessions form a dependency cycle (check depends_on in: {})",
                    cycle_names.join(", ")
                ),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_2_SESSION: &str = r#"
[manifest]
name = "test-manifest"
base_ref = "main"

[[session]]
name = "session-a"
task = "Do thing A"
file_scope = ["src/a/**"]
timeout_secs = 300

[[session]]
name = "session-b"
task = "Do thing B"
file_scope = ["src/b/**"]
"#;

    const VALID_WITH_SCRIPT: &str = r#"
[manifest]
name = "scripted-test"

[[session]]
name = "add-login"
task = "Implement login endpoint"
file_scope = ["src/auth/**", "src/lib.rs"]
timeout_secs = 300

[session.script]
backend = "scripted"

[[session.script.steps]]
action = "commit"
message = "Add login handler"
files = [
  { path = "src/auth/login.rs", content = "pub fn login() {}\n" },
  { path = "src/lib.rs", content = "pub mod auth;\n" },
]

[[session.script.steps]]
action = "commit"
message = "Add login tests"
files = [
  { path = "src/auth/login_test.rs", content = "// tests\n" },
]
"#;

    #[test]
    fn parse_valid_2_session_manifest() {
        let manifest = Manifest::parse(VALID_2_SESSION).expect("should parse");
        assert_eq!(manifest.manifest.name, "test-manifest");
        assert_eq!(manifest.manifest.base_ref, "main");
        assert_eq!(manifest.sessions.len(), 2);
        assert_eq!(manifest.sessions[0].name, "session-a");
        assert_eq!(manifest.sessions[0].task.as_deref(), Some("Do thing A"));
        assert_eq!(manifest.sessions[0].timeout_secs, Some(300));
        assert_eq!(manifest.sessions[1].name, "session-b");
        assert!(manifest.sessions[1].timeout_secs.is_none());
    }

    #[test]
    fn parse_manifest_with_script_steps() {
        let manifest = Manifest::parse(VALID_WITH_SCRIPT).expect("should parse");
        assert_eq!(manifest.sessions.len(), 1);
        let session = &manifest.sessions[0];
        let script = session.script.as_ref().expect("should have script");
        assert_eq!(script.backend, "scripted");
        assert_eq!(script.steps.len(), 2);

        match &script.steps[0] {
            ScriptStep::Commit { message, files } => {
                assert_eq!(message, "Add login handler");
                assert_eq!(files.len(), 2);
                assert_eq!(files[0].path, "src/auth/login.rs");
                assert_eq!(files[0].content.as_deref(), Some("pub fn login() {}\n"));
            }
        }
    }

    #[test]
    fn validate_rejects_empty_sessions() {
        // Construct a Manifest directly with empty sessions and validate it
        let manifest = Manifest {
            manifest: ManifestMeta {
                name: "empty".to_string(),
                base_ref: "HEAD".to_string(),
                merge_strategy: None,
                parallel_by_default: true,
                on_failure: None,
            },
            sessions: vec![],
        };
        let err = manifest.validate().unwrap_err();
        assert!(
            err.to_string().contains("at least one session"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_rejects_duplicate_session_names() {
        let toml = r#"
[manifest]
name = "dupes"

[[session]]
name = "same-name"
task = "First"

[[session]]
name = "same-name"
task = "Second"
"#;
        let err = Manifest::parse(toml).unwrap_err();
        assert!(
            err.to_string().contains("duplicate session name: 'same-name'"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_rejects_session_with_no_task() {
        let toml = r#"
[manifest]
name = "no-task"

[[session]]
name = "missing"
"#;
        let err = Manifest::parse(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("must have either 'task' or 'task_file'"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_rejects_session_with_both_task_and_task_file() {
        let toml = r#"
[manifest]
name = "both"

[[session]]
name = "both-set"
task = "inline"
task_file = "path/to/task.md"
"#;
        let err = Manifest::parse(toml).unwrap_err();
        assert!(
            err.to_string()
                .contains("cannot have both 'task' and 'task_file'"),
            "got: {err}"
        );
    }

    #[test]
    fn round_trip_serialize_deserialize() {
        let manifest = Manifest::parse(VALID_WITH_SCRIPT).expect("should parse");
        let serialized = toml::to_string(&manifest).expect("should serialize");
        let deserialized = Manifest::parse(&serialized).expect("should re-parse");
        assert_eq!(manifest.manifest.name, deserialized.manifest.name);
        assert_eq!(manifest.sessions.len(), deserialized.sessions.len());
    }

    #[test]
    fn parse_failure_mode_variants() {
        let toml = r#"
[manifest]
name = "failure-modes"

[[session]]
name = "crash-session"
task = "Test crash"

[session.script]
backend = "scripted"
exit_after = 1
simulate_failure = "crash"

[[session.script.steps]]
action = "commit"
message = "First commit"
files = [{ path = "a.txt", content = "a" }]
"#;
        let manifest = Manifest::parse(toml).expect("should parse crash");
        let script = manifest.sessions[0].script.as_ref().unwrap();
        assert!(matches!(script.simulate_failure, Some(FailureMode::Crash)));

        let toml_hang = toml.replace("\"crash\"", "\"hang\"");
        let manifest = Manifest::parse(&toml_hang).expect("should parse hang");
        let script = manifest.sessions[0].script.as_ref().unwrap();
        assert!(matches!(script.simulate_failure, Some(FailureMode::Hang)));

        let toml_partial = toml.replace("\"crash\"", "\"partial\"");
        let manifest = Manifest::parse(&toml_partial).expect("should parse partial");
        let script = manifest.sessions[0].script.as_ref().unwrap();
        assert!(matches!(
            script.simulate_failure,
            Some(FailureMode::Partial)
        ));
    }

    #[test]
    fn default_base_ref_is_head() {
        let toml = r#"
[manifest]
name = "defaults"

[[session]]
name = "s1"
task = "task"
"#;
        let manifest = Manifest::parse(toml).expect("should parse");
        assert_eq!(manifest.manifest.base_ref, "HEAD");
    }

    #[test]
    fn validate_rejects_empty_script_steps() {
        let toml = r#"
[manifest]
name = "empty-steps"

[[session]]
name = "bad-script"
task = "something"

[session.script]
backend = "scripted"
steps = []
"#;
        let err = Manifest::parse(toml).unwrap_err();
        assert!(
            err.to_string().contains("at least one step"),
            "got: {err}"
        );
    }

    #[test]
    fn parse_malformed_toml_returns_error() {
        let err = Manifest::parse("this is not { valid toml").unwrap_err();
        assert!(
            matches!(err, SmeltError::ManifestParse(_)),
            "expected ManifestParse, got: {err}"
        );
    }

    #[test]
    fn validate_rejects_invalid_glob_pattern() {
        let toml = r#"
[manifest]
name = "bad-glob"

[[session]]
name = "s1"
task = "something"
file_scope = ["[invalid"]
"#;
        let err = Manifest::parse(toml).unwrap_err();
        assert!(
            err.to_string().contains("invalid glob pattern"),
            "got: {err}"
        );
    }

    #[test]
    fn parse_manifest_with_depends_on() {
        let toml = r#"
[manifest]
name = "deps-test"

[[session]]
name = "base"
task = "Base task"

[[session]]
name = "dependent"
task = "Depends on base"
depends_on = ["base"]
"#;
        let manifest = Manifest::parse(toml).expect("should parse");
        assert!(manifest.sessions[0].depends_on.is_none());
        assert_eq!(
            manifest.sessions[1].depends_on.as_deref(),
            Some(&["base".to_string()][..])
        );
    }

    #[test]
    fn parse_manifest_with_parallel_by_default_false() {
        let toml = r#"
[manifest]
name = "sequential"
parallel_by_default = false

[[session]]
name = "s1"
task = "First"

[[session]]
name = "s2"
task = "Second"
"#;
        let manifest = Manifest::parse(toml).expect("should parse");
        assert!(!manifest.manifest.parallel_by_default);
    }

    #[test]
    fn parse_manifest_with_on_failure_abort() {
        let toml = r#"
[manifest]
name = "abort-test"
on_failure = "abort"

[[session]]
name = "s1"
task = "Task"
"#;
        let manifest = Manifest::parse(toml).expect("should parse");
        assert_eq!(
            manifest.manifest.on_failure,
            Some(FailurePolicy::Abort)
        );
    }

    #[test]
    fn parse_rejects_unknown_on_failure() {
        let toml = r#"
[manifest]
name = "bad-policy"
on_failure = "retry"

[[session]]
name = "s1"
task = "Task"
"#;
        let err = Manifest::parse(toml).unwrap_err();
        // Serde rejects unknown enum variants at parse time
        assert!(
            err.to_string().contains("on_failure"),
            "expected on_failure parse error, got: {err}"
        );
    }

    #[test]
    fn validate_rejects_dangling_depends_on() {
        let toml = r#"
[manifest]
name = "dangling"

[[session]]
name = "s1"
task = "Task"
depends_on = ["nonexistent"]
"#;
        let err = Manifest::parse(toml).unwrap_err();
        assert!(
            err.to_string().contains("depends on unknown session 'nonexistent'"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_rejects_self_dependency() {
        let toml = r#"
[manifest]
name = "self-dep"

[[session]]
name = "s1"
task = "Task"
depends_on = ["s1"]
"#;
        let err = Manifest::parse(toml).unwrap_err();
        assert!(
            err.to_string().contains("cannot depend on itself"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_rejects_dependency_cycle() {
        let toml = r#"
[manifest]
name = "cycle"

[[session]]
name = "a"
task = "Task A"
depends_on = ["b"]

[[session]]
name = "b"
task = "Task B"
depends_on = ["a"]
"#;
        let err = Manifest::parse(toml).unwrap_err();
        assert!(
            err.to_string().contains("dependency cycle"),
            "got: {err}"
        );
    }

    #[test]
    fn parallel_by_default_true_is_default() {
        let toml = r#"
[manifest]
name = "defaults"

[[session]]
name = "s1"
task = "Task"
"#;
        let manifest = Manifest::parse(toml).expect("should parse");
        assert!(manifest.manifest.parallel_by_default);
    }

    #[test]
    fn parse_session_with_env_vars() {
        let toml = r#"
[manifest]
name = "env-test"

[[session]]
name = "with-env"
task = "task with env"

[session.env]
FOO = "bar"
BAZ = "qux"
"#;
        let manifest = Manifest::parse(toml).expect("should parse");
        let env = manifest.sessions[0].env.as_ref().expect("should have env");
        assert_eq!(env.get("FOO").unwrap(), "bar");
        assert_eq!(env.get("BAZ").unwrap(), "qux");
    }
}
