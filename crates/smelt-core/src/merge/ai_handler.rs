//! AI-powered conflict handler that resolves merge conflicts via LLM.

use std::path::Path;
use std::sync::Arc;

use tracing::{info, warn};

use crate::ai::{AiConfig, AiProvider, build_resolution_prompt, build_system_prompt};
use crate::error::{Result, SmeltError};
use crate::git::GitOps;
use crate::merge::conflict::ConflictScan;
use crate::merge::types::{ConflictAction, ResolutionMethod};
use crate::merge::ConflictHandler;

/// AI conflict handler that resolves merge conflicts by calling an LLM per file.
///
/// Extracts 3-way merge context (base/ours/theirs) via git index stages,
/// builds structured prompts, and writes the LLM's resolved output to disk.
///
/// This is a single-attempt resolver. Retry logic with user feedback is
/// handled at the CLI layer (Plan 03's `AiInteractiveConflictHandler`).
pub struct AiConflictHandler<G: GitOps, P: AiProvider + 'static> {
    git: G,
    provider: Arc<P>,
    config: AiConfig,
    target_branch: String,
}

impl<G: GitOps, P: AiProvider + 'static> AiConflictHandler<G, P> {
    /// Create a new AI conflict handler.
    ///
    /// `provider` is `Arc<P>` so the CLI layer can share the same provider
    /// instance for retry calls that bypass this handler.
    pub fn new(git: G, provider: Arc<P>, config: AiConfig, target_branch: String) -> Self {
        Self {
            git,
            provider,
            config,
            target_branch,
        }
    }
}

impl<G: GitOps + Send + Sync, P: AiProvider + 'static> ConflictHandler for AiConflictHandler<G, P> {
    async fn handle_conflict(
        &self,
        session_name: &str,
        files: &[String],
        _scan: &ConflictScan,
        work_dir: &Path,
    ) -> Result<ConflictAction> {
        let model = self
            .config
            .model
            .as_deref()
            .unwrap_or_else(|| default_model_for_provider(&self.config));

        // Get commit subjects for context (non-fatal if it fails).
        let commit_subjects = self
            .git
            .log_subjects(&format!("{}..MERGE_HEAD", &self.target_branch))
            .await
            .unwrap_or_else(|e| {
                warn!("failed to get commit subjects for AI context: {e}");
                Vec::new()
            });

        let system_prompt = build_system_prompt();

        info!(
            "AI resolving {} conflicted file(s) in session '{session_name}' with model '{model}'",
            files.len()
        );

        for file in files {
            // Extract 3-way context via git index stages.
            // Stage 1 = base (may not exist for new files).
            let base = match self.git.show_index_stage(work_dir, 1, file).await {
                Ok(content) => content,
                Err(e) => {
                    // Stage 1 absent is expected for new-file conflicts (no common ancestor).
                    tracing::debug!("stage 1 (base) not available for '{file}': {e}");
                    String::new()
                }
            };
            let ours = self
                .git
                .show_index_stage(work_dir, 2, file)
                .await
                .map_err(|e| SmeltError::AiResolution {
                    message: format!("failed to read ours (stage 2) for '{file}': {e}"),
                })?;
            let theirs = self
                .git
                .show_index_stage(work_dir, 3, file)
                .await
                .map_err(|e| SmeltError::AiResolution {
                    message: format!("failed to read theirs (stage 3) for '{file}': {e}"),
                })?;

            let prompt = build_resolution_prompt(
                file,
                &base,
                &ours,
                &theirs,
                session_name,
                None, // task_description — accepted v0.1.0 limitation
                &commit_subjects,
            );

            let resolved = self
                .provider
                .complete(model, system_prompt, &prompt)
                .await
                .map_err(|e| {
                    warn!("AI resolution failed for '{file}': {e}");
                    SmeltError::AiResolution {
                        message: format!("failed to resolve '{file}': {e}"),
                    }
                })?;

            tokio::fs::write(work_dir.join(file), &resolved)
                .await
                .map_err(|e| SmeltError::AiResolution {
                    message: format!("failed to write resolved content for '{file}': {e}"),
                })?;

            info!("AI resolved '{file}'");
        }

        Ok(ConflictAction::Resolved(ResolutionMethod::AiAssisted))
    }
}

/// Return a sensible default model name based on the configured provider.
pub fn default_model_for_provider(config: &AiConfig) -> &'static str {
    match config.provider.as_deref() {
        Some("anthropic") => "claude-sonnet-4-20250514",
        Some("openai") => "gpt-4o",
        Some("ollama") => "llama3.1",
        Some("gemini") | Some("google") => "gemini-2.0-flash",
        _ => "claude-sonnet-4-20250514", // default to Anthropic
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge::conflict::ConflictScan;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    /// Mock AiProvider that returns predictable content.
    struct MockProvider {
        response: String,
        call_count: AtomicUsize,
    }

    impl MockProvider {
        fn new(response: &str) -> Self {
            Self {
                response: response.to_owned(),
                call_count: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    impl AiProvider for MockProvider {
        async fn complete(
            &self,
            _model: &str,
            _system_prompt: &str,
            _user_prompt: &str,
        ) -> crate::Result<String> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(self.response.clone())
        }
    }

    /// Mock AiProvider that always fails.
    struct FailingProvider;

    impl AiProvider for FailingProvider {
        async fn complete(
            &self,
            _model: &str,
            _system_prompt: &str,
            _user_prompt: &str,
        ) -> crate::Result<String> {
            Err(SmeltError::AiResolution {
                message: "mock provider failure".to_owned(),
            })
        }
    }

    /// Minimal mock GitOps for testing AI handler.
    #[derive(Clone)]
    struct MockGitOps;

    impl GitOps for MockGitOps {
        async fn repo_root(&self) -> crate::Result<std::path::PathBuf> {
            Ok(std::path::PathBuf::from("/mock"))
        }
        async fn is_inside_work_tree(&self, _path: &std::path::Path) -> crate::Result<bool> {
            Ok(true)
        }
        async fn current_branch(&self) -> crate::Result<String> {
            Ok("main".to_owned())
        }
        async fn head_short(&self) -> crate::Result<String> {
            Ok("abc1234".to_owned())
        }
        async fn worktree_add(
            &self,
            _path: &std::path::Path,
            _branch: &str,
            _start: &str,
        ) -> crate::Result<()> {
            Ok(())
        }
        async fn worktree_remove(
            &self,
            _path: &std::path::Path,
            _force: bool,
        ) -> crate::Result<()> {
            Ok(())
        }
        async fn worktree_list(
            &self,
        ) -> crate::Result<Vec<crate::worktree::GitWorktreeEntry>> {
            Ok(vec![])
        }
        async fn worktree_add_existing(
            &self,
            _path: &std::path::Path,
            _branch: &str,
        ) -> crate::Result<()> {
            Ok(())
        }
        async fn worktree_prune(&self) -> crate::Result<()> {
            Ok(())
        }
        async fn worktree_is_dirty(&self, _path: &std::path::Path) -> crate::Result<bool> {
            Ok(false)
        }
        async fn branch_delete(
            &self,
            _name: &str,
            _force: bool,
        ) -> crate::Result<()> {
            Ok(())
        }
        async fn branch_is_merged(
            &self,
            _branch: &str,
            _base: &str,
        ) -> crate::Result<bool> {
            Ok(false)
        }
        async fn branch_exists(&self, _name: &str) -> crate::Result<bool> {
            Ok(false)
        }
        async fn add(
            &self,
            _work_dir: &std::path::Path,
            _paths: &[&str],
        ) -> crate::Result<()> {
            Ok(())
        }
        async fn commit(
            &self,
            _work_dir: &std::path::Path,
            _msg: &str,
        ) -> crate::Result<String> {
            Ok("commit123".to_owned())
        }
        async fn rev_list_count(
            &self,
            _branch: &str,
            _base: &str,
        ) -> crate::Result<usize> {
            Ok(1)
        }
        async fn merge_base(&self, _a: &str, _b: &str) -> crate::Result<String> {
            Ok("base123".to_owned())
        }
        async fn branch_create(
            &self,
            _name: &str,
            _start: &str,
        ) -> crate::Result<()> {
            Ok(())
        }
        async fn merge_squash(
            &self,
            _work_dir: &std::path::Path,
            _source: &str,
        ) -> crate::Result<()> {
            Ok(())
        }
        async fn unmerged_files(
            &self,
            _work_dir: &std::path::Path,
        ) -> crate::Result<Vec<String>> {
            Ok(vec![])
        }
        async fn reset_hard(
            &self,
            _work_dir: &std::path::Path,
            _target: &str,
        ) -> crate::Result<()> {
            Ok(())
        }
        async fn rev_parse(&self, _rev: &str) -> crate::Result<String> {
            Ok("abc123".to_owned())
        }
        async fn diff_name_only(
            &self,
            _base: &str,
            _head: &str,
        ) -> crate::Result<Vec<String>> {
            Ok(vec![])
        }
        async fn log_subjects(&self, _range: &str) -> crate::Result<Vec<String>> {
            Ok(vec!["test commit".to_owned()])
        }
        async fn diff_numstat(
            &self,
            _from: &str,
            _to: &str,
        ) -> crate::Result<Vec<(usize, usize, String)>> {
            Ok(vec![])
        }
        async fn show_index_stage(
            &self,
            _work_dir: &std::path::Path,
            stage: u8,
            _file: &str,
        ) -> crate::Result<String> {
            match stage {
                1 => Ok("base content".to_owned()),
                2 => Ok("ours content".to_owned()),
                3 => Ok("theirs content".to_owned()),
                _ => Err(SmeltError::AiResolution {
                    message: "invalid stage".to_owned(),
                }),
            }
        }
    }

    #[tokio::test]
    async fn handle_conflict_writes_resolved_files() {
        let tmp = TempDir::new().unwrap();
        let work_dir = tmp.path();

        // Create a conflicted file
        let file_name = "conflict.rs";
        std::fs::write(
            work_dir.join(file_name),
            "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>>",
        )
        .unwrap();

        let resolved_content = "merged content";
        let provider = Arc::new(MockProvider::new(resolved_content));
        let config = AiConfig::default();

        let handler = AiConflictHandler::new(
            MockGitOps,
            Arc::clone(&provider),
            config,
            "main".to_owned(),
        );

        let scan = ConflictScan {
            hunks: vec![],
            total_conflict_lines: 0,
        };

        let result = handler
            .handle_conflict("test-session", &[file_name.to_owned()], &scan, work_dir)
            .await;

        assert!(result.is_ok());
        assert!(matches!(
            result.unwrap(),
            ConflictAction::Resolved(ResolutionMethod::AiAssisted)
        ));

        // Verify the file was written with resolved content
        let written = std::fs::read_to_string(work_dir.join(file_name)).unwrap();
        assert_eq!(written, resolved_content);

        // Provider was called once per file
        assert_eq!(provider.calls(), 1);
    }

    #[tokio::test]
    async fn handle_conflict_propagates_provider_error() {
        let tmp = TempDir::new().unwrap();
        let work_dir = tmp.path();

        std::fs::write(work_dir.join("file.rs"), "conflicted").unwrap();

        let provider = Arc::new(FailingProvider);
        let config = AiConfig::default();

        let handler = AiConflictHandler::new(
            MockGitOps,
            provider,
            config,
            "main".to_owned(),
        );

        let scan = ConflictScan {
            hunks: vec![],
            total_conflict_lines: 0,
        };

        let result = handler
            .handle_conflict("test-session", &["file.rs".to_owned()], &scan, work_dir)
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SmeltError::AiResolution { .. }));
    }

    #[test]
    fn default_model_anthropic() {
        let config = AiConfig {
            provider: Some("anthropic".to_owned()),
            ..AiConfig::default()
        };
        assert_eq!(default_model_for_provider(&config), "claude-sonnet-4-20250514");
    }

    #[test]
    fn default_model_openai() {
        let config = AiConfig {
            provider: Some("openai".to_owned()),
            ..AiConfig::default()
        };
        assert_eq!(default_model_for_provider(&config), "gpt-4o");
    }

    #[test]
    fn default_model_ollama() {
        let config = AiConfig {
            provider: Some("ollama".to_owned()),
            ..AiConfig::default()
        };
        assert_eq!(default_model_for_provider(&config), "llama3.1");
    }

    #[test]
    fn default_model_gemini() {
        let config = AiConfig {
            provider: Some("gemini".to_owned()),
            ..AiConfig::default()
        };
        assert_eq!(default_model_for_provider(&config), "gemini-2.0-flash");
    }

    #[test]
    fn default_model_google() {
        let config = AiConfig {
            provider: Some("google".to_owned()),
            ..AiConfig::default()
        };
        assert_eq!(default_model_for_provider(&config), "gemini-2.0-flash");
    }

    #[test]
    fn default_model_none_falls_back_to_anthropic() {
        let config = AiConfig::default();
        assert_eq!(default_model_for_provider(&config), "claude-sonnet-4-20250514");
    }

    #[test]
    fn default_model_unknown_falls_back_to_anthropic() {
        let config = AiConfig {
            provider: Some("custom-provider".to_owned()),
            ..AiConfig::default()
        };
        assert_eq!(default_model_for_provider(&config), "claude-sonnet-4-20250514");
    }
}
