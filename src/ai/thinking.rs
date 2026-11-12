use serde_json::{Value, json};

use crate::state::{AiProvider, AiSettings, OpenAiApiMode};

#[derive(Clone, Copy)]
enum Control {
    OpenAi,
    Typed,
    Claude,
    Gemini,
    Ollama,
}

// Only enable controls with a known on/off API contract. In particular,
// older OpenAI reasoning models and Gemini Pro cannot disable thinking.
// https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.1
// https://api-docs.deepseek.com/guides/thinking_mode/
// https://platform.claude.com/docs/en/build-with-claude/extended-thinking
// https://ai.google.dev/gemini-api/docs/generate-content/thinking
pub(super) fn uses_responses(settings: &AiSettings) -> bool {
    settings.provider.supports_openai_api_mode()
        && settings.openai_api_mode == OpenAiApiMode::Responses
}

fn matches_model(model: &str, name: &str) -> bool {
    model == name
        || model.strip_prefix(name).is_some_and(|suffix| {
            suffix.starts_with('-')
                && suffix[1..]
                    .chars()
                    .all(|ch| ch.is_ascii_digit() || ch == '-')
        })
}

fn control(settings: &AiSettings) -> Option<Control> {
    let model = settings.model.trim().to_ascii_lowercase();
    match settings.provider {
        AiProvider::DeepSeek => Some(Control::Typed),
        AiProvider::OpenAi | AiProvider::OpenAiCompatible => {
            if !uses_responses(settings)
                && (model.starts_with("mimo-") || model.starts_with("deepseek-"))
            {
                Some(Control::Typed)
            } else if matches_model(&model, "gpt-5.1") {
                Some(Control::OpenAi)
            } else {
                None
            }
        }
        AiProvider::Claude => {
            if model == "claude-sonnet-4-20250514"
                || model == "claude-opus-4-20250514"
                || [
                    "claude-3-7-sonnet",
                    "claude-sonnet-4-5",
                    "claude-sonnet-4-6",
                    "claude-opus-4-5",
                    "claude-opus-4-6",
                    "claude-haiku-4-5",
                ]
                .iter()
                .any(|name| matches_model(&model, name))
            {
                Some(Control::Claude)
            } else {
                None
            }
        }
        AiProvider::Gemini if model.starts_with("gemini-2.5-flash") => Some(Control::Gemini),
        AiProvider::Ollama if model.starts_with("qwen3") => Some(Control::Ollama),
        _ => None,
    }
}

pub fn supports_thinking_toggle(settings: &AiSettings) -> bool {
    control(settings).is_some()
}

pub(super) fn parameters(settings: &AiSettings) -> Value {
    let enabled = settings.thinking_enabled;
    match control(settings) {
        Some(Control::OpenAi) if uses_responses(settings) => {
            json!({"reasoning": {"effort": if enabled { "medium" } else { "none" }}})
        }
        Some(Control::OpenAi | Control::Ollama) => {
            json!({"reasoning_effort": if enabled { "medium" } else { "none" }})
        }
        Some(Control::Typed) => {
            json!({"thinking": {"type": if enabled { "enabled" } else { "disabled" }}})
        }
        Some(Control::Claude) if enabled => {
            json!({"thinking": {"type": "enabled", "budget_tokens": 1024}})
        }
        Some(Control::Claude) => json!({"thinking": {"type": "disabled"}}),
        Some(Control::Gemini) => json!({"generationConfig": {"thinkingConfig": {
            "thinkingBudget": if enabled { 1024 } else { 0 },
            "includeThoughts": enabled,
        }}}),
        None => json!({}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sends_provider_specific_on_and_off_parameters() {
        let cases = [
            (
                AiProvider::DeepSeek,
                "deepseek-v4-pro",
                json!({"thinking":{"type":"enabled"}}),
                json!({"thinking":{"type":"disabled"}}),
            ),
            (
                AiProvider::OpenAiCompatible,
                "mimo-v2.5-pro",
                json!({"thinking":{"type":"enabled"}}),
                json!({"thinking":{"type":"disabled"}}),
            ),
            (
                AiProvider::Claude,
                "claude-sonnet-4-20250514",
                json!({"thinking":{"type":"enabled","budget_tokens":1024}}),
                json!({"thinking":{"type":"disabled"}}),
            ),
            (
                AiProvider::Gemini,
                "gemini-2.5-flash",
                json!({"generationConfig":{"thinkingConfig":{"thinkingBudget":1024,"includeThoughts":true}}}),
                json!({"generationConfig":{"thinkingConfig":{"thinkingBudget":0,"includeThoughts":false}}}),
            ),
            (
                AiProvider::Ollama,
                "qwen3:8b",
                json!({"reasoning_effort":"medium"}),
                json!({"reasoning_effort":"none"}),
            ),
        ];
        for (provider, model, enabled, disabled) in cases {
            let mut settings = AiSettings {
                provider,
                model: model.into(),
                ..Default::default()
            };
            assert!(supports_thinking_toggle(&settings));
            assert_eq!(parameters(&settings), enabled);
            settings.thinking_enabled = false;
            assert_eq!(parameters(&settings), disabled);
        }
    }

    #[test]
    fn honors_responses_mode_for_both_openai_providers() {
        for provider in [AiProvider::OpenAi, AiProvider::OpenAiCompatible] {
            let mut settings = AiSettings {
                provider,
                model: "gpt-5.1".into(),
                ..Default::default()
            };
            assert_eq!(parameters(&settings), json!({"reasoning_effort":"medium"}));
            settings.openai_api_mode = OpenAiApiMode::Responses;
            settings.thinking_enabled = false;
            assert!(uses_responses(&settings));
            assert_eq!(
                parameters(&settings),
                json!({"reasoning":{"effort":"none"}})
            );
        }
    }

    #[test]
    fn leaves_unsupported_models_at_provider_defaults() {
        for (provider, model) in [
            (AiProvider::OpenAi, "gpt-4.1-mini"),
            (AiProvider::OpenAiCompatible, "custom-model"),
            (AiProvider::Gemini, "gemini-2.5-pro"),
            (AiProvider::Claude, "claude-sonnet-4-7"),
            (AiProvider::Ollama, "llama3.1"),
        ] {
            let settings = AiSettings {
                provider,
                model: model.into(),
                thinking_enabled: false,
                ..Default::default()
            };
            assert!(!supports_thinking_toggle(&settings));
            assert_eq!(parameters(&settings), json!({}));
        }
    }

    #[test]
    fn rig_serialization_preserves_thinking_and_structured_output() {
        let mut settings = AiSettings {
            model: "gpt-5.1".into(),
            thinking_enabled: false,
            ..Default::default()
        };
        let request = |settings: &AiSettings| rig::completion::CompletionRequest {
            model: None,
            preamble: None,
            chat_history: rig::OneOrMany::one(rig::message::Message::user("Connection test")),
            documents: vec![],
            tools: vec![],
            temperature: None,
            max_tokens: Some(4096),
            tool_choice: None,
            additional_params: Some(parameters(settings)),
            output_schema: Some(schemars::schema_for!(crate::ai::GeneratedTaskList)),
        };
        let chat = rig::providers::openai::completion::CompletionRequest::try_from((
            settings.model.clone(),
            request(&settings),
        ))
        .unwrap();
        let chat = serde_json::to_value(chat).unwrap();
        assert_eq!(chat["reasoning_effort"], "none");
        assert_eq!(chat["response_format"]["type"], "json_schema");

        settings.openai_api_mode = OpenAiApiMode::Responses;
        let responses = rig::providers::openai::responses_api::CompletionRequest::try_from((
            settings.model.clone(),
            request(&settings),
        ))
        .unwrap();
        let responses = serde_json::to_value(responses).unwrap();
        assert_eq!(responses["reasoning"]["effort"], "none");
        assert_eq!(responses["text"]["format"]["type"], "json_schema");

        settings.provider = AiProvider::Gemini;
        settings.model = "gemini-2.5-flash".into();
        let gemini: rig::providers::gemini::completion::gemini_api_types::AdditionalParameters =
            serde_json::from_value(parameters(&settings)).unwrap();
        let gemini = serde_json::to_value(gemini).unwrap();
        assert_eq!(
            gemini["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            0
        );
    }
}
