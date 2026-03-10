//! AI provider abstraction for LLM-assisted conflict resolution.

mod prompt;
mod provider;

use std::future::Future;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub use prompt::{build_resolution_prompt, build_retry_prompt, build_system_prompt};
pub use provider::GenAiProvider;

/// Configuration for AI-assisted conflict resolution.
///
/// Loaded from the `[ai]` section of `.smelt/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// Whether AI resolution is enabled (default: true).
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Provider name (e.g. "anthropic", "openai", "ollama").
    /// When `None`, genai infers the provider from the model name prefix.
    pub provider: Option<String>,

    /// Model override. When `None`, the caller picks a sensible default.
    pub model: Option<String>,

    /// Maximum retry attempts with user feedback before falling back to manual
    /// resolution (default: 2).
    #[serde(default = "default_max_retries")]
    pub max_retries: u8,

    /// API key fallback. Prefer environment variables (`ANTHROPIC_API_KEY`,
    /// `OPENAI_API_KEY`, etc.) — this field is a last-resort override.
    #[serde(skip_serializing)]
    pub api_key: Option<String>,

    /// Custom endpoint URL for proxies or self-hosted providers.
    ///
    /// **Not yet implemented** — reserved for future use with genai's client
    /// builder. Setting this field currently has no effect.
    pub endpoint: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_max_retries() -> u8 {
    2
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: None,
            model: None,
            max_retries: 2,
            api_key: None,
            endpoint: None,
        }
    }
}

/// Wrapper for TOML parsing of `.smelt/config.toml`.
#[derive(Deserialize)]
struct ConfigFile {
    ai: Option<AiConfig>,
}

impl AiConfig {
    /// Load AI configuration from `.smelt/config.toml`.
    ///
    /// Returns `None` if the file is unreadable or has no `[ai]` section.
    pub fn load(smelt_dir: &Path) -> Option<AiConfig> {
        let config_path = smelt_dir.join("config.toml");
        let content = match std::fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => {
                tracing::warn!(
                    "failed to read {}: {e}",
                    config_path.display()
                );
                return None;
            }
        };
        match toml::from_str::<ConfigFile>(&content) {
            Ok(config_file) => config_file.ai,
            Err(e) => {
                tracing::warn!(
                    "failed to parse {}: {e}",
                    config_path.display()
                );
                None
            }
        }
    }
}

/// Trait for sending prompts to an LLM and receiving text responses.
///
/// Uses RPITIT (return-position `impl Trait` in trait) — no `async-trait`
/// crate required.
pub trait AiProvider: Send + Sync {
    /// Send a system + user prompt to the LLM and return the text response.
    fn complete(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> impl Future<Output = crate::Result<String>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let config = AiConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_retries, 2);
        assert!(config.provider.is_none());
        assert!(config.model.is_none());
        assert!(config.api_key.is_none());
        assert!(config.endpoint.is_none());
    }

    #[test]
    fn load_returns_none_for_missing_file() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        assert!(AiConfig::load(tmp.path()).is_none());
    }

    #[test]
    fn load_returns_none_for_no_ai_section() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        std::fs::write(tmp.path().join("config.toml"), "version = 1\n").unwrap();
        assert!(AiConfig::load(tmp.path()).is_none());
    }

    #[test]
    fn load_parses_ai_section() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let toml = r#"
version = 1

[ai]
enabled = true
provider = "anthropic"
model = "claude-sonnet-4-20250514"
max_retries = 3
"#;
        std::fs::write(tmp.path().join("config.toml"), toml).unwrap();
        let config = AiConfig::load(tmp.path()).expect("should parse ai section");
        assert!(config.enabled);
        assert_eq!(config.provider.as_deref(), Some("anthropic"));
        assert_eq!(config.model.as_deref(), Some("claude-sonnet-4-20250514"));
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn load_uses_defaults_for_missing_fields() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let toml = r#"
version = 1

[ai]
provider = "openai"
"#;
        std::fs::write(tmp.path().join("config.toml"), toml).unwrap();
        let config = AiConfig::load(tmp.path()).expect("should parse ai section");
        assert!(config.enabled); // default true
        assert_eq!(config.max_retries, 2); // default 2
    }
}
