use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{NaiveDate, NaiveTime};
use futures::StreamExt;
use rig::{
    agent::MultiTurnStreamItem,
    client::CompletionClient,
    completion::{CompletionModel, GetTokenUsage},
    providers::{anthropic, gemini, openai},
    streaming::{StreamedAssistantContent, StreamingPrompt, ToolCallDeltaContent},
};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};

use crate::state::{AiProvider, AiSettings, DueDate, OpenAiApiMode};

const GENERATE_TASKS_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECTION_TEST_TIMEOUT: Duration = Duration::from_secs(20);
const MODEL_LIST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_GENERATED_TASKS: usize = 20;

#[derive(Debug, Clone)]
pub struct GeneratedSubtask {
    pub title: String,
    pub details: Option<String>,
    pub due_date: Option<DueDate>,
    pub is_starred: bool,
}

#[derive(Debug, Clone)]
pub struct GeneratedTask {
    pub title: String,
    pub details: Option<String>,
    pub due_date: Option<DueDate>,
    pub is_starred: bool,
    pub subtasks: Vec<GeneratedSubtask>,
}

#[derive(Debug, Clone)]
pub enum AiStreamEvent {
    Text(String),
    Reasoning(String),
    ToolCall(String),
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
    #[serde(default)]
    is_starred: bool,
}

pub fn generate_tasks_streaming(
    settings: &AiSettings,
    description: &str,
    mut on_event: impl FnMut(AiStreamEvent),
) -> Result<Vec<GeneratedTask>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create AI runtime")?;
    runtime.block_on(async {
        tokio::time::timeout(
            GENERATE_TASKS_TIMEOUT,
            generate_tasks_streaming_async(settings, description, &mut on_event),
        )
        .await
        .context("AI request timed out")?
    })
}

fn generate_tasks_with_timeout(
    settings: &AiSettings,
    description: &str,
    timeout: Duration,
) -> Result<Vec<GeneratedTask>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("create AI runtime")?;
    runtime.block_on(async {
        tokio::time::timeout(timeout, generate_tasks_async(settings, description))
            .await
            .context("AI request timed out")?
    })
}

pub fn test_connection(settings: &AiSettings) -> Result<()> {
    let mut settings = settings.clone();
    settings.custom_prompt.clear();
    generate_tasks_with_timeout(
        &settings,
        "Create exactly one simple task titled Connection test.",
        CONNECTION_TEST_TIMEOUT,
    )
    .map(|_| ())
}

pub fn preview_generated_tasks_from_stream(raw: &str) -> Vec<GeneratedTask> {
    if let Ok(list) = parse_generated_task_list(raw) {
        return normalize_generated_tasks(list).unwrap_or_default();
    }

    extract_complete_task_objects(clean_streamed_json(raw))
        .into_iter()
        .filter_map(|task_json| serde_json::from_str::<AiTask>(&task_json).ok())
        .filter_map(normalize_ai_task)
        .take(MAX_GENERATED_TASKS)
        .collect()
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

async fn generate_tasks_streaming_async(
    settings: &AiSettings,
    description: &str,
    on_event: &mut impl FnMut(AiStreamEvent),
) -> Result<Vec<GeneratedTask>> {
    let prompt = build_task_prompt(description);

    let extracted = match settings.provider {
        AiProvider::OpenAi if settings.openai_api_mode == OpenAiApiMode::Responses => {
            let client = openai::Client::builder()
                .api_key(settings.api_key.trim())
                .base_url(normalize_endpoint(&settings.endpoint))
                .build()
                .context("build OpenAI client")?;
            extract_streaming(
                &client,
                &settings.model,
                &prompt,
                &settings.custom_prompt,
                on_event,
            )
            .await?
        }
        AiProvider::OpenAi | AiProvider::OpenAiCompatible | AiProvider::DeepSeek => {
            let client = openai::CompletionsClient::builder()
                .api_key(settings.api_key.trim())
                .base_url(normalize_endpoint(&settings.endpoint))
                .build()
                .context("build OpenAI-compatible client")?;
            extract_streaming(
                &client,
                &settings.model,
                &prompt,
                &settings.custom_prompt,
                on_event,
            )
            .await?
        }
        AiProvider::Ollama => {
            let client = openai::CompletionsClient::builder()
                .api_key(settings.api_key.trim())
                .base_url(normalize_endpoint(&settings.endpoint))
                .build()
                .context("build Ollama client")?;
            extract_streaming(
                &client,
                &settings.model,
                &prompt,
                &settings.custom_prompt,
                on_event,
            )
            .await?
        }
        AiProvider::Claude => {
            let client = anthropic::Client::builder()
                .api_key(settings.api_key.trim())
                .base_url(normalize_endpoint(&settings.endpoint))
                .build()
                .context("build Anthropic client")?;
            extract_streaming(
                &client,
                &settings.model,
                &prompt,
                &settings.custom_prompt,
                on_event,
            )
            .await?
        }
        AiProvider::Gemini => {
            let client = gemini::Client::builder()
                .api_key(settings.api_key.trim())
                .base_url(normalize_endpoint(&settings.endpoint))
                .build()
                .context("build Gemini client")?;
            extract_streaming(
                &client,
                &settings.model,
                &prompt,
                &settings.custom_prompt,
                on_event,
            )
            .await?
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

async fn extract_streaming<C>(
    client: &C,
    model: &str,
    prompt: &str,
    custom_prompt: &str,
    on_event: &mut impl FnMut(AiStreamEvent),
) -> Result<GeneratedTaskList>
where
    C: CompletionClient,
    C::CompletionModel: 'static,
    <C::CompletionModel as CompletionModel>::StreamingResponse:
        Clone + Unpin + GetTokenUsage + Send,
{
    let preamble = build_streaming_preamble(custom_prompt);
    let agent = client
        .agent(model)
        .preamble(&preamble)
        .max_tokens(4096)
        .output_schema::<GeneratedTaskList>()
        .build();
    let mut stream = agent.stream_prompt(prompt.to_string()).await;
    let mut raw = String::new();

    while let Some(chunk) = stream.next().await {
        let MultiTurnStreamItem::StreamAssistantItem(chunk) =
            chunk.context("stream generated tasks")?
        else {
            continue;
        };

        match chunk {
            StreamedAssistantContent::Text(text) => {
                raw.push_str(&text.text);
                on_event(AiStreamEvent::Text(text.text));
            }
            StreamedAssistantContent::ReasoningDelta { reasoning, .. } => {
                on_event(AiStreamEvent::Reasoning(reasoning));
            }
            StreamedAssistantContent::Reasoning(reasoning) => {
                on_event(AiStreamEvent::Reasoning(format!("{reasoning:?}")));
            }
            StreamedAssistantContent::ToolCall { tool_call, .. } => {
                on_event(AiStreamEvent::ToolCall(tool_call.function.name));
            }
            StreamedAssistantContent::ToolCallDelta { content, .. } => match content {
                ToolCallDeltaContent::Name(name) => on_event(AiStreamEvent::ToolCall(name)),
                ToolCallDeltaContent::Delta(delta) => on_event(AiStreamEvent::ToolCall(delta)),
            },
            StreamedAssistantContent::Final(_) => {}
        }
    }

    parse_generated_task_list(&raw).context("parse streamed generated tasks")
}

fn build_extractor_preamble(custom_prompt: &str) -> String {
    let mut preamble = String::from("Generate a todo plan. Never invent completed tasks.");
    if !custom_prompt.trim().is_empty() {
        preamble.push_str("\n\nAdditional user instructions:\n");
        preamble.push_str(custom_prompt.trim());
    }
    preamble
}

fn build_streaming_preamble(custom_prompt: &str) -> String {
    let mut preamble = String::from(
        "Generate a todo plan. Never invent completed tasks.\n\
         Return only valid JSON. Do not wrap the JSON in Markdown fences. Do not include commentary.",
    );
    if !custom_prompt.trim().is_empty() {
        preamble.push_str("\n\nAdditional user instructions:\n");
        preamble.push_str(custom_prompt.trim());
    }
    preamble
}

fn build_task_prompt(description: &str) -> String {
    let today = chrono::Local::now();
    let schema = serde_json::to_string_pretty(&schema_for!(GeneratedTaskList))
        .unwrap_or_else(|_| "{}".to_string());

    format!(
        "Create a practical todo plan from the user's description.\n\
         Current local date and time: {}.\n\
         Return at most {MAX_GENERATED_TASKS} top-level tasks.\n\
         Titles must be concise and non-empty. Put supporting context in details.\n\
         Use subtasks only when they make the parent task meaningfully actionable.\n\
         due_date must be YYYY-MM-DD or null. due_time must be HH:MM in 24-hour time or null.\n\
         Set is_starred only for genuinely important or urgent top-level tasks.\n\n\
         Return JSON matching this schema:\n{}\n\n\
         User description:\n{}",
        today.format("%Y-%m-%d %H:%M %:z"),
        schema,
        description.trim()
    )
}

fn parse_generated_task_list(raw: &str) -> Result<GeneratedTaskList> {
    let cleaned = clean_streamed_json(raw);
    serde_json::from_str(cleaned).context("decode streamed JSON")
}

fn clean_streamed_json(raw: &str) -> &str {
    let trimmed = raw.trim();
    let Some(stripped) = trimmed.strip_prefix("```") else {
        return trimmed;
    };

    let stripped = stripped
        .strip_prefix("json")
        .or_else(|| stripped.strip_prefix("JSON"))
        .unwrap_or(stripped)
        .trim_start();
    stripped
        .strip_suffix("```")
        .map(str::trim_end)
        .unwrap_or(stripped)
}

fn normalize_generated_tasks(list: GeneratedTaskList) -> Result<Vec<GeneratedTask>> {
    let tasks = list
        .tasks
        .into_iter()
        .take(MAX_GENERATED_TASKS)
        .filter_map(normalize_ai_task)
        .collect::<Vec<_>>();

    if tasks.is_empty() {
        bail!("no valid tasks returned");
    }
    Ok(tasks)
}

fn normalize_ai_task(task: AiTask) -> Option<GeneratedTask> {
    let title = task.title.trim().to_string();
    if title.is_empty() {
        return None;
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
                is_starred: subtask.is_starred,
            })
        })
        .collect();
    Some(GeneratedTask {
        title,
        details: clean_optional(task.details),
        due_date: parse_due_date(task.due_date, task.due_time),
        is_starred: task.is_starred,
        subtasks,
    })
}

fn extract_complete_task_objects(raw: &str) -> Vec<String> {
    let Some(tasks_key) = raw.find("\"tasks\"") else {
        return Vec::new();
    };
    let Some(array_start_offset) = raw[tasks_key..].find('[') else {
        return Vec::new();
    };
    let array = &raw[tasks_key + array_start_offset + 1..];

    let mut objects = Vec::new();
    let mut object_start = None;
    let mut brace_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in array.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if brace_depth == 0 {
                    object_start = Some(index);
                }
                brace_depth += 1;
            }
            '}' if brace_depth > 0 => {
                brace_depth -= 1;
                if brace_depth == 0 {
                    if let Some(start) = object_start.take() {
                        objects.push(array[start..=index].to_string());
                        if objects.len() >= MAX_GENERATED_TASKS {
                            break;
                        }
                    }
                }
            }
            ']' if brace_depth == 0 => break,
            _ => {}
        }
    }

    objects
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
        .timeout_global(Some(MODEL_LIST_TIMEOUT))
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
                        is_starred: false,
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

    #[test]
    fn parses_streamed_json_with_markdown_fence() {
        let parsed = parse_generated_task_list(
            r#"```json
{"tasks":[{"title":"Plan move","details":null,"due_date":null,"due_time":null,"is_starred":false,"subtasks":[]}]}
```"#,
        )
        .unwrap();

        assert_eq!(parsed.tasks.len(), 1);
        assert_eq!(parsed.tasks[0].title, "Plan move");
    }

    #[test]
    fn previews_complete_tasks_from_partial_stream() {
        let tasks = preview_generated_tasks_from_stream(
            r#"{"tasks":[{"title":"Confirm moving date","details":"Call the movers","due_date":"2026-06-25","due_time":"09:00","is_starred":true,"subtasks":[]},{"title":"Pack kitchen""#,
        );

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Confirm moving date");
        assert!(tasks[0].is_starred);
    }
}
