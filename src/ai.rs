use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::state::{AiProvider, AiSettings};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Deserialize)]
struct ModelList {
    data: Vec<Model>,
}

#[derive(Deserialize)]
struct Model {
    id: String,
}

pub fn fetch_models(settings: &AiSettings) -> Result<Vec<String>> {
    if settings.endpoint.is_empty() {
        bail!("endpoint is empty");
    }

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .into();

    let mut models = match settings.provider {
        AiProvider::Claude => fetch_claude_models(&agent, settings)?,
        AiProvider::Gemini => bail!("provider does not support model listing"),
        AiProvider::OpenAi
        | AiProvider::OpenAiCompatible
        | AiProvider::DeepSeek
        | AiProvider::Ollama => fetch_openai_compatible_models(&agent, settings)?,
    };

    models.retain(|model| !model.trim().is_empty());
    models.sort_by_key(|model| model.to_lowercase());
    models.dedup();
    Ok(models)
}

fn fetch_openai_compatible_models(
    agent: &ureq::Agent,
    settings: &AiSettings,
) -> Result<Vec<String>> {
    let url = append_path(normalize_endpoint(&settings.endpoint), "models");
    let mut request = agent.get(&url);
    if !settings.api_key.is_empty() {
        request = request.header("Authorization", format!("Bearer {}", settings.api_key));
    }

    let mut response = request.call().with_context(|| format!("request {url}"))?;
    let response: ModelList = response
        .body_mut()
        .read_json()
        .context("parse model list")?;
    Ok(response.data.into_iter().map(|model| model.id).collect())
}

fn fetch_claude_models(agent: &ureq::Agent, settings: &AiSettings) -> Result<Vec<String>> {
    let base = normalize_endpoint(&settings.endpoint);
    let url = if base.ends_with("/v1") {
        append_path(base, "models")
    } else {
        append_path(base, "v1/models")
    };
    let mut response = agent
        .get(&url)
        .header("x-api-key", &settings.api_key)
        .header("anthropic-version", "2023-06-01")
        .call()
        .with_context(|| format!("request {url}"))?;
    let response: ModelList = response
        .body_mut()
        .read_json()
        .context("parse model list")?;
    Ok(response.data.into_iter().map(|model| model.id).collect())
}

fn normalize_endpoint(endpoint: &str) -> &str {
    let endpoint = endpoint.trim().trim_end_matches('/');
    [
        "/chat/completions",
        "/responses",
        "/messages",
        "/v1/chat/completions",
        "/v1/responses",
        "/v1/messages",
    ]
    .into_iter()
    .find_map(|suffix| endpoint.strip_suffix(suffix))
    .unwrap_or(endpoint)
    .trim_end_matches('/')
}

fn append_path(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_completion_endpoints() {
        assert_eq!(
            normalize_endpoint("https://api.openai.com/v1/chat/completions"),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            normalize_endpoint("https://api.anthropic.com/v1/messages/"),
            "https://api.anthropic.com/v1"
        );
    }

    #[test]
    fn appends_paths_without_double_slashes() {
        assert_eq!(
            append_path("https://api.example.com/v1/", "/models"),
            "https://api.example.com/v1/models"
        );
    }
}
