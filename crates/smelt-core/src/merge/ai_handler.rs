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
            let base = self
                .git
                .show_index_stage(work_dir, 1, file)
                .await
                .unwrap_or_default();
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
fn default_model_for_provider(config: &AiConfig) -> &'static str {
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
