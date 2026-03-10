//! GenAI-backed provider implementation.

use genai::chat::{ChatMessage, ChatRequest};
use genai::Client;

use super::AiConfig;
use super::AiProvider;
use crate::error::SmeltError;

/// LLM provider backed by the [`genai`] crate.
///
/// Wraps a [`genai::Client`] and maps errors to [`SmeltError::AiResolution`].
pub struct GenAiProvider {
    client: Client,
}

impl GenAiProvider {
    /// Create a new provider from the given configuration.
    ///
    /// If `config.api_key` is set and `config.provider` is known, the key is
    /// injected into the corresponding environment variable so that genai's
    /// built-in auth resolution picks it up.
    pub fn new(config: &AiConfig) -> crate::Result<Self> {
        // If the user provided an API key in config, set the appropriate env var
        // so genai's default AuthResolver finds it.
        if let (Some(key), Some(provider)) = (&config.api_key, &config.provider) {
            let env_name = provider_to_env_key(provider);
            if let Some(env_name) = env_name {
                // Only set if not already set — env vars take precedence over config.
                if std::env::var(env_name).is_err() {
                    // SAFETY: Environment variable mutation is not thread-safe.
                    // This call runs once during handler construction (not in a
                    // hot loop), targets provider-specific env vars that no other
                    // thread reads at this point, and is skipped if the var is
                    // already set. Future improvement: move env setup to a
                    // pre-runtime sync context.
                    unsafe { std::env::set_var(env_name, key) };
                }
            }
        }

        let client = Client::default();

        Ok(Self { client })
    }
}

impl AiProvider for GenAiProvider {
    async fn complete(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> crate::Result<String> {
        let chat_req = ChatRequest::new(vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_prompt),
        ]);

        let chat_res = self
            .client
            .exec_chat(model, chat_req, None)
            .await
            .map_err(|e| SmeltError::AiResolution {
                message: e.to_string(),
            })?;

        let text = chat_res
            .first_text()
            .ok_or_else(|| SmeltError::AiResolution {
                message: "LLM returned an empty response".to_owned(),
            })?;

        Ok(strip_code_fences(text))
    }
}

/// Map a provider name to its expected API key environment variable.
fn provider_to_env_key(provider: &str) -> Option<&'static str> {
    match provider.to_lowercase().as_str() {
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        "openai" => Some("OPENAI_API_KEY"),
        "gemini" | "google" => Some("GEMINI_API_KEY"),
        "cohere" => Some("COHERE_API_KEY"),
        "groq" => Some("GROQ_API_KEY"),
        "xai" => Some("XAI_API_KEY"),
        "deepseek" => Some("DEEPSEEK_API_KEY"),
        _ => None, // ollama, custom endpoints — no key needed
    }
}

/// Strip markdown code fences that LLMs often add despite instructions.
///
/// Handles:
/// - `` ```lang\n...\n``` ``
/// - `` ```\n...\n``` ``
/// - No fences (passthrough)
fn strip_code_fences(s: &str) -> String {
    let trimmed = s.trim();

    // Must start with ``` to be a fenced block.
    if !trimmed.starts_with("```") {
        return s.to_owned();
    }

    // Find the end of the opening fence line.
    let after_opening = match trimmed.find('\n') {
        Some(pos) => pos + 1,
        None => return s.to_owned(), // Just "```" with nothing else — passthrough.
    };

    // Must end with ``` (possibly followed by whitespace).
    let body = &trimmed[after_opening..];
    let body_trimmed = body.trim_end();
    if !body_trimmed.ends_with("```") {
        return s.to_owned(); // Opening fence but no closing fence — passthrough.
    }

    // Strip the closing fence.
    let content = &body_trimmed[..body_trimmed.len() - 3];
    content.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_no_fences() {
        let input = "fn main() {\n    println!(\"hello\");\n}";
        assert_eq!(strip_code_fences(input), input);
    }

    #[test]
    fn strip_fences_with_language() {
        let input = "```rust\nfn main() {\n    println!(\"hello\");\n}\n```";
        assert_eq!(
            strip_code_fences(input),
            "fn main() {\n    println!(\"hello\");\n}"
        );
    }

    #[test]
    fn strip_fences_without_language() {
        let input = "```\nsome content\n```";
        assert_eq!(strip_code_fences(input), "some content");
    }

    #[test]
    fn strip_fences_with_extra_whitespace() {
        let input = "  ```rust\nfn main() {}\n```  ";
        assert_eq!(strip_code_fences(input), "fn main() {}");
    }

    #[test]
    fn strip_no_closing_fence() {
        let input = "```rust\nfn main() {}";
        assert_eq!(strip_code_fences(input), input);
    }

    #[test]
    fn provider_to_env_key_known_providers() {
        assert_eq!(provider_to_env_key("anthropic"), Some("ANTHROPIC_API_KEY"));
        assert_eq!(provider_to_env_key("openai"), Some("OPENAI_API_KEY"));
        assert_eq!(provider_to_env_key("Anthropic"), Some("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn provider_to_env_key_unknown() {
        assert_eq!(provider_to_env_key("ollama"), None);
        assert_eq!(provider_to_env_key("custom"), None);
    }
}
