use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{NaiveDate, NaiveTime};
use rig::{
    client::CompletionClient,
    providers::{anthropic, gemini, openai},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::{AiProvider, AiSettings, DueDate, OpenAiApiMode};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_GENERATED_TASKS: usize = 20;

#[derive(Debug, Clone)]
pub struct GeneratedSubtask {
    pub title: String,
    pub details: Option<String>,
    pub due_date: Option<DueDate>,
}

#[derive(Debug, Clone)]
pub struct GeneratedTask {
    pub title: String,
    pub details: Option<String>,
    pub due_date: Option<DueDate>,
    pub is_starred: bool,
    pub subtasks: Vec<GeneratedSubtask>,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
struct GeneratedTaskList {
    tasks: Vec<AiTask>,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
struct AiTask {
    title: String,
    #[serde(default)]
    details: Option<String>,
    #[serde(default)]
    due_date: Option<String>,
    #[serde(default)]
    due_time: Option<String>,
    #[serde(default)]
    is_starred: bool,
    #[serde(default)]
    subtasks: Vec<AiSubtask>,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
struct AiSubtask {
    title: String,
    #[serde(default)]
    details: Option<String>,
    #[serde(default)]
    due_date: Option<String>,
    #[serde(default)]
    due_time: Option<String>,
}

pub fn generate_tasks(settings: &AiSettings, description: &str) -> Result<Vec<GeneratedTask>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create AI runtime")?;
    runtime.block_on(async {
        tokio::time::timeout(REQUEST_TIMEOUT, generate_tasks_async(settings, description))
            .await
            .context("AI request timed out")?
    })
}

pub fn test_connection(settings: &AiSettings) -> Result<()> {
    let mut settings = settings.clone();
    settings.custom_prompt.clear();
    generate_tasks(
        &settings,
        "Create exactly one simple task titled Connection test.",
    )
    .map(|_| ())
}

async fn generate_tasks_async(
    settings: &AiSettings,
    description: &str,
) -> Result<Vec<GeneratedTask>> {
    let today = chrono::Local::now();
    let prompt = format!(
        "Create a practical todo plan from the user's description.\n\
         Current local date and time: {}.\n\
         Return at most {MAX_GENERATED_TASKS} top-level tasks.\n\
         Titles must be concise and non-empty. Put supporting context in details.\n\
         Use subtasks only when they make the parent task meaningfully actionable.\n\
         due_date must be YYYY-MM-DD or null. due_time must be HH:MM in 24-hour time or null.\n\
         Set is_starred only for genuinely important or urgent top-level tasks.\n\n\
         User description:\n{}",
        today.format("%Y-%m-%d %H:%M %:z"),
        description.trim()
    );

    let extracted = match settings.provider {
        AiProvider::OpenAi if settings.openai_api_mode == OpenAiApiMode::Responses => {
            let client = openai::Client::builder()
                .api_key(settings.api_key.trim())
                .base_url(normalize_endpoint(&settings.endpoint))
                .build()
                .context("build OpenAI client")?;
            extract(&client, &settings.model, &prompt, &settings.custom_prompt).await?
        }
        AiProvider::OpenAi | AiProvider::OpenAiCompatible | AiProvider::DeepSeek => {
            let client = openai::CompletionsClient::builder()
                .api_key(settings.api_key.trim())
                .base_url(normalize_endpoint(&settings.endpoint))
                .build()
                .context("build OpenAI-compatible client")?;
            extract(&client, &settings.model, &prompt, &settings.custom_prompt).await?
        }
        AiProvider::Ollama => {
            let client = openai::CompletionsClient::builder()
                .api_key(settings.api_key.trim())
                .base_url(normalize_endpoint(&settings.endpoint))
                .build()
                .context("build Ollama client")?;
            extract(&client, &settings.model, &prompt, &settings.custom_prompt).await?
        }
        AiProvider::Claude => {
            let client = anthropic::Client::builder()
                .api_key(settings.api_key.trim())
                .base_url(normalize_endpoint(&settings.endpoint))
                .build()
                .context("build Anthropic client")?;
            extract(&client, &settings.model, &prompt, &settings.custom_prompt).await?
        }
        AiProvider::Gemini => {
            let client = gemini::Client::builder()
                .api_key(settings.api_key.trim())
                .base_url(normalize_endpoint(&settings.endpoint))
                .build()
                .context("build Gemini client")?;
            extract(&client, &settings.model, &prompt, &settings.custom_prompt).await?
        }
    };

    normalize_generated_tasks(extracted)
}

async fn extract<C>(
    client: &C,
    model: &str,
    prompt: &str,
    custom_prompt: &str,
) -> Result<GeneratedTaskList>
where
    C: CompletionClient,
{
    let preamble = build_extractor_preamble(custom_prompt);

    client
        .extractor::<GeneratedTaskList>(model)
        .preamble(&preamble)
        .max_tokens(4096)
        .retries(1)
        .build()
        .extract(prompt)
        .await
        .context("extract generated tasks")
}

fn build_extractor_preamble(custom_prompt: &str) -> String {
    let mut preamble = String::from("Generate a todo plan. Never invent completed tasks.");
    if !custom_prompt.trim().is_empty() {
        preamble.push_str("\n\nAdditional user instructions:\n");
        preamble.push_str(custom_prompt.trim());
    }
    preamble
}

fn normalize_generated_tasks(list: GeneratedTaskList) -> Result<Vec<GeneratedTask>> {
    let mut tasks = Vec::new();
    for task in list.tasks.into_iter().take(MAX_GENERATED_TASKS) {
        let title = task.title.trim().to_string();
        if title.is_empty() {
            continue;
        }
        let subtasks = task
            .subtasks
            .into_iter()
            .filter_map(|subtask| {
                let title = subtask.title.trim().to_string();
                (!title.is_empty()).then(|| GeneratedSubtask {
                    title,
                    details: clean_optional(subtask.details),
                    due_date: parse_due_date(subtask.due_date, subtask.due_time),
                })
            })
            .collect();
        tasks.push(GeneratedTask {
            title,
            details: clean_optional(task.details),
            due_date: parse_due_date(task.due_date, task.due_time),
            is_starred: task.is_starred,
            subtasks,
        });
    }

    if tasks.is_empty() {
        bail!("no valid tasks returned");
    }
    Ok(tasks)
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn parse_due_date(date: Option<String>, time: Option<String>) -> Option<DueDate> {
    let date = NaiveDate::parse_from_str(date?.trim(), "%Y-%m-%d").ok()?;
    let time = time.and_then(|value| NaiveTime::parse_from_str(value.trim(), "%H:%M").ok());
    Some(DueDate::new(date, time))
}

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

    #[test]
    fn parses_generated_due_date_and_time() {
        assert_eq!(
            parse_due_date(Some("2026-06-16".to_string()), Some("09:30".to_string())),
            Some(DueDate::new(
                NaiveDate::from_ymd_opt(2026, 6, 16).unwrap(),
                NaiveTime::from_hms_opt(9, 30, 0)
            ))
        );
        assert_eq!(parse_due_date(Some("not-a-date".to_string()), None), None);
    }

    #[test]
    fn removes_empty_generated_tasks() {
        let tasks = normalize_generated_tasks(GeneratedTaskList {
            tasks: vec![
                AiTask {
                    title: "  ".to_string(),
                    details: None,
                    due_date: None,
                    due_time: None,
                    is_starred: false,
                    subtasks: vec![],
                },
                AiTask {
                    title: "Book venue".to_string(),
                    details: Some("  Compare three options.  ".to_string()),
                    due_date: None,
                    due_time: None,
                    is_starred: true,
                    subtasks: vec![AiSubtask {
                        title: "  ".to_string(),
                        details: None,
                        due_date: None,
                        due_time: None,
                    }],
                },
            ],
        })
        .unwrap();

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Book venue");
        assert_eq!(tasks[0].details.as_deref(), Some("Compare three options."));
        assert!(tasks[0].is_starred);
        assert!(tasks[0].subtasks.is_empty());
    }

    #[test]
    fn appends_custom_prompt_to_extractor_preamble() {
        let default = build_extractor_preamble("  ");
        assert_eq!(
            default,
            "Generate a todo plan. Never invent completed tasks."
        );

        let customized = build_extractor_preamble("  Respond in Chinese.  ");
        assert!(customized.ends_with("Additional user instructions:\nRespond in Chinese."));
    }
}
