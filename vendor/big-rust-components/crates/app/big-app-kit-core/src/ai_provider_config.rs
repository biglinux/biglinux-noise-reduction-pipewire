// SPDX-License-Identifier: MIT

//! AI provider config spec.
//!
//! Captures the data side of the provider configuration dialog (API
//! base URL, model id, temperature, max tokens, keyring label that
//! holds the API key). The HTTP transport itself lives in the app.

/// Configuration container describing big ai provider runtime settings.
#[derive(Debug, Clone, PartialEq)]
pub struct BigAiProviderConfig {
    /// Stable provider id (matches `BigAiProviderSpec::id`).
    pub provider_id: String,
    /// Default model id (`gpt-5`, `claude-opus-4-7`, `qwen3:32b`).
    pub model: String,
    /// API base URL.
    pub base_url: String,
    /// Sampling temperature in `0.0..=2.0`.
    pub temperature: f32,
    /// Maximum response tokens. `0` means provider default.
    pub max_tokens: u32,
    /// Streaming responses on the wire.
    pub streaming: bool,
    /// Keyring label holding the API key (looked up via
    /// [`big_os_kit::keyring::BigKeyringSecretStore`]).
    pub api_key_label: Option<String>,
}

impl BigAiProviderConfig {
    /// Build a config with sensible defaults.
    #[must_use]
    pub fn new(
        provider_id: impl Into<String>,
        model: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            provider_id: provider_id.into(),
            model: model.into(),
            base_url: base_url.into(),
            temperature: 0.7,
            max_tokens: 0,
            streaming: true,
            api_key_label: None,
        }
    }

    /// Override temperature; clamped to `0.0..=2.0`.
    #[must_use]
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature.clamp(0.0, 2.0);
        self
    }

    /// Override max-tokens cap.
    #[must_use]
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Disable streaming.
    #[must_use]
    pub fn without_streaming(mut self) -> Self {
        self.streaming = false;
        self
    }

    /// Attach a keyring label for the API key.
    #[must_use]
    pub fn with_api_key_label(mut self, label: impl Into<String>) -> Self {
        self.api_key_label = Some(label.into());
        self
    }

    /// Validate base URL prefix and model emptiness.
    ///
    /// # Errors
    /// Returns a description string when validation fails.
    pub fn validate(&self) -> Result<(), String> {
        if self.provider_id.is_empty() {
            return Err("provider_id is empty".into());
        }
        if self.model.is_empty() {
            return Err("model is empty".into());
        }
        if !(self.base_url.starts_with("http://")
            || self.base_url.starts_with("https://")
            || self.base_url.starts_with("file://"))
        {
            return Err(format!(
                "base_url uses unsupported scheme: {}",
                self.base_url
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_provider_config() -> BigAiProviderConfig {
        BigAiProviderConfig::new("openai", "gpt-5", "https://api.openai.com/v1")
    }

    #[test]
    fn defaults_are_streaming_with_provider_default_max_tokens() {
        let provider_config = sample_provider_config();
        assert!(provider_config.streaming);
        assert_eq!(provider_config.max_tokens, 0);
        assert!((provider_config.temperature - 0.7).abs() < 1e-3);
    }

    #[test]
    fn temperature_is_clamped() {
        let provider_config = sample_provider_config().with_temperature(5.0);
        assert!((provider_config.temperature - 2.0).abs() < 1e-3);
        let provider_config = sample_provider_config().with_temperature(-1.0);
        assert!(provider_config.temperature.abs() < 1e-3);
    }

    #[test]
    fn validate_requires_scheme_and_fields() {
        sample_provider_config().validate().unwrap();
        let mut provider_config = sample_provider_config();
        provider_config.base_url = "api.openai.com".into();
        assert!(provider_config.validate().is_err());
        provider_config.base_url = "https://api.openai.com".into();
        provider_config.model.clear();
        assert!(provider_config.validate().is_err());
    }

    #[test]
    fn keyring_label_attaches() {
        let provider_config = sample_provider_config().with_api_key_label("ai.openai.key");
        assert_eq!(
            provider_config.api_key_label.as_deref(),
            Some("ai.openai.key")
        );
    }
}
