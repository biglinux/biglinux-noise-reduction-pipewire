// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Built-in AI provider catalog used by the shared assistant UI.

use super::BigAiProviderSpec;

/// Default provider selected when no app setting exists yet.
pub const DEFAULT_AI_PROVIDER_ID: &str = "groq";
/// Default base URL for local OpenAI-compatible runtimes.
pub const DEFAULT_LOCAL_AI_BASE_URL: &str = "http://localhost:11434/v1";

const BUILTIN_AI_PROVIDER_CATALOG: &[BuiltinAiProviderCatalogEntry] = &[
    BuiltinAiProviderCatalogEntry::remote(
        "groq",
        "Groq",
        "llama-3.1-8b-instant",
        "https://console.groq.com/keys",
        "https://api.groq.com/openai/v1",
    ),
    BuiltinAiProviderCatalogEntry::remote(
        "gemini",
        "Gemini",
        "gemini-2.5-flash",
        "https://aistudio.google.com/app/apikey",
        "https://generativelanguage.googleapis.com/v1beta",
    ),
    BuiltinAiProviderCatalogEntry::remote(
        "openrouter",
        "OpenRouter",
        "openrouter/polaris-alpha",
        "https://openrouter.ai/keys",
        "https://openrouter.ai/api/v1",
    ),
    BuiltinAiProviderCatalogEntry::remote(
        "cerebras",
        "Cerebras",
        "llama-3.3-70b",
        "https://cloud.cerebras.ai/platform/api-keys",
        "https://api.cerebras.ai/v1",
    ),
    BuiltinAiProviderCatalogEntry::remote(
        "github",
        "GitHub Models",
        "gpt-4o-mini",
        "https://github.com/settings/tokens",
        "https://models.inference.ai.azure.com",
    ),
    BuiltinAiProviderCatalogEntry::remote(
        "mistral",
        "Mistral AI",
        "mistral-small-latest",
        "https://console.mistral.ai/api-keys/",
        "https://api.mistral.ai/v1",
    ),
    BuiltinAiProviderCatalogEntry::local(
        "local",
        "Local (Ollama / LM Studio)",
        "llama3.2",
        DEFAULT_LOCAL_AI_BASE_URL,
    ),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BuiltinAiProviderCatalogEntry {
    id: &'static str,
    label: &'static str,
    default_model: &'static str,
    api_key_url: Option<&'static str>,
    model_browser_base_url: &'static str,
    local: bool,
}

impl BuiltinAiProviderCatalogEntry {
    const fn remote(
        id: &'static str,
        label: &'static str,
        default_model: &'static str,
        api_key_url: &'static str,
        model_browser_base_url: &'static str,
    ) -> Self {
        Self {
            id,
            label,
            default_model,
            api_key_url: Some(api_key_url),
            model_browser_base_url,
            local: false,
        }
    }

    const fn local(
        id: &'static str,
        label: &'static str,
        default_model: &'static str,
        model_browser_base_url: &'static str,
    ) -> Self {
        Self {
            id,
            label,
            default_model,
            api_key_url: None,
            model_browser_base_url,
            local: true,
        }
    }

    fn provider_spec(self) -> BigAiProviderSpec {
        if self.local {
            BigAiProviderSpec::local(self.id, self.label, self.default_model)
        } else {
            BigAiProviderSpec::remote(
                self.id,
                self.label,
                self.default_model,
                self.api_key_url.unwrap_or(""),
            )
        }
    }
}

/// Return the built-in BigLinux AI provider catalog.
#[must_use]
pub fn builtin_ai_provider_specs() -> Vec<BigAiProviderSpec> {
    BUILTIN_AI_PROVIDER_CATALOG
        .iter()
        .map(|entry| entry.provider_spec())
        .collect()
}

/// Find one built-in provider by stable provider id.
#[must_use]
pub fn builtin_ai_provider_spec(provider_id: &str) -> Option<BigAiProviderSpec> {
    BUILTIN_AI_PROVIDER_CATALOG
        .iter()
        .copied()
        .find(|entry| entry.id == provider_id)
        .map(BuiltinAiProviderCatalogEntry::provider_spec)
}

/// Return the built-in provider's position in [`builtin_ai_provider_specs`].
#[must_use]
pub fn builtin_ai_provider_index(provider_id: &str) -> usize {
    BUILTIN_AI_PROVIDER_CATALOG
        .iter()
        .position(|entry| entry.id == provider_id)
        .unwrap_or(0)
}

/// Resolve the default model for a provider id.
#[must_use]
pub fn builtin_ai_provider_default_model(provider_id: &str) -> &'static str {
    BUILTIN_AI_PROVIDER_CATALOG
        .iter()
        .find(|entry| entry.id == provider_id)
        .map_or(BUILTIN_AI_PROVIDER_CATALOG[0].default_model, |entry| {
            entry.default_model
        })
}

/// Resolve the OpenAI-compatible base URL used by the model browser.
#[must_use]
pub fn builtin_ai_provider_model_browser_base_url(
    provider_id: &str,
    local_base_url: &str,
) -> String {
    BUILTIN_AI_PROVIDER_CATALOG
        .iter()
        .find(|entry| entry.id == provider_id)
        .map_or_else(String::new, |entry| {
            if entry.local {
                local_base_url.trim().to_owned()
            } else {
                entry.model_browser_base_url.to_owned()
            }
        })
}
