use anyhow::Result;
use chrono::NaiveTime;
use gpui::{App, AppContext, Bounds, Context, Entity, Global, Pixels};
use gpui_component::ThemeMode;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use super::persistence::{
    Format, Loaded, Persistence, SaveKind, SaveResult, load_file, save_file, wait_for_save,
};
use crate::helpers::get_or_create_config_path;

const LIGHT_THEME_MODE: &str = "light";
const DARK_THEME_MODE: &str = "dark";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    OpenAi,
    OpenAiCompatible,
    Claude,
    Gemini,
    DeepSeek,
    Ollama,
}

impl AiProvider {
    pub const ALL: [Self; 6] = [
        Self::OpenAi,
        Self::OpenAiCompatible,
        Self::Claude,
        Self::Gemini,
        Self::DeepSeek,
        Self::Ollama,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::OpenAiCompatible => "OpenAI Compatible",
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
            Self::DeepSeek => "DeepSeek",
            Self::Ollama => "Ollama",
        }
    }

    pub const fn default_endpoint(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1",
            Self::OpenAiCompatible => "",
            Self::Claude => "https://api.anthropic.com",
            Self::Gemini => "https://generativelanguage.googleapis.com",
            Self::DeepSeek => "https://api.deepseek.com/v1",
            Self::Ollama => "http://localhost:11434/v1",
        }
    }

    pub const fn default_model(self) -> &'static str {
        match self {
            Self::OpenAi => "gpt-4.1-mini",
            Self::OpenAiCompatible => "",
            Self::Claude => "claude-sonnet-4-20250514",
            Self::Gemini => "gemini-2.5-flash",
            Self::DeepSeek => "deepseek-chat",
            Self::Ollama => "llama3.1",
        }
    }

    pub const fn requires_api_key(self) -> bool {
        !matches!(self, Self::Ollama)
    }

    pub const fn supports_openai_api_mode(self) -> bool {
        matches!(self, Self::OpenAi | Self::OpenAiCompatible)
    }

    pub const fn supports_model_listing(self) -> bool {
        !matches!(self, Self::Gemini)
    }
}

impl Default for AiProvider {
    fn default() -> Self {
        Self::OpenAi
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiApiMode {
    ChatCompletions,
    Responses,
}

impl Default for OpenAiApiMode {
    fn default() -> Self {
        Self::ChatCompletions
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiSettings {
    #[serde(default)]
    pub provider: AiProvider,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_ai_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_ai_model")]
    pub model: String,
    #[serde(default)]
    pub openai_api_mode: OpenAiApiMode,
    #[serde(default = "default_true")]
    pub thinking_enabled: bool,
    #[serde(default)]
    pub custom_prompt: String,
}

fn default_ai_endpoint() -> String {
    AiProvider::default().default_endpoint().to_string()
}

fn default_ai_model() -> String {
    AiProvider::default().default_model().to_string()
}

impl Default for AiSettings {
    fn default() -> Self {
        let provider = AiProvider::default();
        Self {
            provider,
            api_key: String::new(),
            endpoint: provider.default_endpoint().to_string(),
            model: provider.default_model().to_string(),
            openai_api_mode: OpenAiApiMode::default(),
            thinking_enabled: true,
            custom_prompt: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    HideToTray,
    Quit,
}

impl Default for CloseBehavior {
    fn default() -> Self {
        Self::HideToTray
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultView {
    LastOpened,
    AllTasks,
    Starred,
    FirstGroup,
}

impl Default for DefaultView {
    fn default() -> Self {
        Self::LastOpened
    }
}

const fn default_true() -> bool {
    true
}

fn default_no_time_reminder() -> NaiveTime {
    NaiveTime::from_hms_opt(9, 0, 0).expect("valid default notification time")
}

fn default_before_due_minutes() -> Vec<i64> {
    vec![15]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_no_time_reminder")]
    pub default_no_time_reminder: NaiveTime,
    #[serde(default = "default_before_due_minutes")]
    pub before_due_minutes: Vec<i64>,
    #[serde(default = "default_no_time_reminder")]
    pub overdue_daily_time: NaiveTime,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            default_no_time_reminder: default_no_time_reminder(),
            before_due_minutes: default_before_due_minutes(),
            overdue_daily_time: default_no_time_reminder(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TideStatus {
    // group status
    edit_group_id: Option<String>,
    create_group: bool,

    // task status
    show_add_task_btn: bool,
    edit_task_id: Option<String>,
    task_calendar_open: bool,

    // subtask status
    adding_subtask_for: Option<String>,
    edit_subtask_id: Option<String>,
}

impl TideStatus {
    pub fn edit_group_id(&self) -> Option<String> {
        self.edit_group_id.clone()
    }

    pub fn set_edit_group_id(&mut self, editing_group_id: Option<String>) {
        self.edit_group_id = editing_group_id;
    }

    pub fn create_group(&self) -> bool {
        self.create_group
    }

    pub fn set_create_group(&mut self, create_group: bool) {
        self.create_group = create_group;
    }

    pub fn show_add_task_btn(&self) -> bool {
        self.show_add_task_btn
    }

    pub fn set_show_add_task_btn(&mut self, show_add_task_btn: bool) {
        self.show_add_task_btn = show_add_task_btn;
    }

    pub fn edit_task_id(&self) -> Option<String> {
        self.edit_task_id.clone()
    }

    pub fn set_edit_task_id(&mut self, edit_task_id: Option<String>) {
        self.edit_task_id = edit_task_id;
    }

    pub fn task_calendar_open(&self) -> bool {
        self.task_calendar_open
    }

    pub fn set_task_calendar_open(&mut self, task_calendar_open: bool) {
        self.task_calendar_open = task_calendar_open;
    }

    pub fn adding_subtask_for(&self) -> Option<String> {
        self.adding_subtask_for.clone()
    }

    pub fn set_adding_subtask_for(&mut self, parent_id: Option<String>) {
        self.adding_subtask_for = parent_id;
    }

    pub fn edit_subtask_id(&self) -> Option<String> {
        self.edit_subtask_id.clone()
    }

    pub fn set_edit_subtask_id(&mut self, edit_subtask_id: Option<String>) {
        self.edit_subtask_id = edit_subtask_id;
    }
}

impl Default for TideStatus {
    fn default() -> Self {
        Self {
            edit_group_id: None,
            create_group: false,
            show_add_task_btn: true,
            edit_task_id: None,
            task_calendar_open: false,
            adding_subtask_for: None,
            edit_subtask_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tide {
    pub locale: Option<String>,
    bounds: Option<Bounds<Pixels>>,
    theme: Option<String>,
    #[serde(default)]
    launch_at_login: bool,
    #[serde(default = "default_true")]
    show_main_window_on_startup: bool,
    #[serde(default)]
    close_behavior: CloseBehavior,
    #[serde(default)]
    default_view: DefaultView,
    #[serde(default)]
    completed_expanded_by_default: bool,
    #[serde(default)]
    auto_check_updates: bool,
    #[serde(default)]
    notifications: NotificationSettings,
    #[serde(default)]
    ai: AiSettings,
    #[serde(skip)]
    status: TideStatus,
}

impl Default for Tide {
    fn default() -> Self {
        Self {
            locale: None,
            bounds: None,
            theme: None,
            launch_at_login: false,
            show_main_window_on_startup: true,
            close_behavior: CloseBehavior::default(),
            default_view: DefaultView::default(),
            completed_expanded_by_default: false,
            auto_check_updates: false,
            notifications: NotificationSettings::default(),
            ai: AiSettings::default(),
            status: TideStatus::default(),
        }
    }
}

impl Tide {
    pub fn theme(&self) -> Option<ThemeMode> {
        match self.theme.as_deref() {
            Some(LIGHT_THEME_MODE) => Some(ThemeMode::Light),
            Some(DARK_THEME_MODE) => Some(ThemeMode::Dark),
            _ => None,
        }
    }

    pub fn locale(&self) -> &str {
        self.locale.as_deref().unwrap_or("en")
    }

    pub fn launch_at_login(&self) -> bool {
        self.launch_at_login
    }

    pub fn show_main_window_on_startup(&self) -> bool {
        self.show_main_window_on_startup
    }

    pub fn close_behavior(&self) -> CloseBehavior {
        self.close_behavior
    }

    pub fn default_view(&self) -> DefaultView {
        self.default_view
    }

    pub fn completed_expanded_by_default(&self) -> bool {
        self.completed_expanded_by_default
    }

    pub fn auto_check_updates(&self) -> bool {
        self.auto_check_updates
    }

    pub fn notifications(&self) -> &NotificationSettings {
        &self.notifications
    }

    pub fn ai(&self) -> &AiSettings {
        &self.ai
    }

    pub fn status(&self) -> &TideStatus {
        &self.status
    }

    pub fn set_theme(&mut self, theme: Option<ThemeMode>) {
        match theme {
            Some(ThemeMode::Light) => self.theme = Some(LIGHT_THEME_MODE.to_string()),
            Some(ThemeMode::Dark) => self.theme = Some(DARK_THEME_MODE.to_string()),
            _ => self.theme = None,
        }
    }

    pub fn set_locale(&mut self, locale: String) {
        self.locale = Some(locale);
    }

    pub fn set_launch_at_login(&mut self, enabled: bool) {
        self.launch_at_login = enabled;
    }

    pub fn set_show_main_window_on_startup(&mut self, enabled: bool) {
        self.show_main_window_on_startup = enabled;
    }

    pub fn set_close_behavior(&mut self, behavior: CloseBehavior) {
        self.close_behavior = behavior;
    }

    pub fn set_default_view(&mut self, view: DefaultView) {
        self.default_view = view;
    }

    pub fn set_completed_expanded_by_default(&mut self, expanded: bool) {
        self.completed_expanded_by_default = expanded;
    }

    pub fn set_auto_check_updates(&mut self, enabled: bool) {
        self.auto_check_updates = enabled;
    }

    pub fn set_notifications_enabled(&mut self, enabled: bool) {
        self.notifications.enabled = enabled;
    }

    pub fn set_ai(&mut self, settings: AiSettings) {
        self.ai = settings;
    }
}

pub fn save_config(config: &Tide) -> Result<()> {
    let path = get_or_create_config_path()?;
    save_file(&path, config, Format::Toml)
}

pub fn load_config() -> Result<Loaded<Tide>> {
    let path = get_or_create_config_path()?;
    load_file(&path, Format::Toml)
}

#[derive(Debug, Clone)]
pub struct TideStore {
    entity: Entity<Tide>,
}

impl TideStore {
    pub fn new(entity: Entity<Tide>) -> Self {
        Self { entity }
    }

    pub fn read<'a>(&self, cx: &'a App) -> &'a Tide {
        self.entity.read(cx)
    }

    pub fn update<R, C: AppContext>(
        &self,
        cx: &mut C,
        f: impl FnOnce(&mut Tide, &mut Context<Tide>) -> R,
    ) -> R {
        self.entity.update(cx, f)
    }
}

impl Global for TideStore {}

#[inline]
pub fn update_and_save<F>(cx: &mut App, action_name: &'static str, mutation: F)
where
    F: FnOnce(&mut Tide, &App) + Send + 'static + Clone,
{
    update_and_save_with_callback(cx, action_name, mutation, |_, _| {});
}

pub fn update_and_save_with_callback<F>(
    cx: &mut App,
    action_name: &'static str,
    mutation: F,
    on_saved: impl FnOnce(SaveResult, &mut App) + 'static,
) where
    F: FnOnce(&mut Tide, &App) + Send + 'static + Clone,
{
    let store = cx.global::<TideStore>().clone();
    let current = store.update(cx, |tide, cx| {
        mutation(tide, cx);
        cx.notify();
        tide.clone()
    });
    let ai = current.ai().clone();
    let (revision, receipt) = cx
        .global_mut::<Persistence>()
        .enqueue_config(ai, move || save_config(&current));
    cx.refresh_windows();
    cx.spawn(async move |cx| {
        let result = wait_for_save(receipt).await;
        if let Err(error) = &result {
            error!(%error, action = action_name, "Failed to save config");
        } else {
            info!(action = action_name, "Config saved successfully");
        }
        cx.update(|cx| {
            cx.global_mut::<Persistence>()
                .record(SaveKind::Config, revision, &result);
            on_saved(result, cx);
            cx.refresh_windows();
        });
    })
    .detach();
}

#[inline]
pub fn update_status<F>(cx: &App, mutation: F)
where
    F: FnOnce(&mut TideStatus, &App) + Send + 'static + Clone,
{
    let store = cx.global::<TideStore>().clone();
    cx.spawn(async move |cx| {
        let _ = store.update(cx, |tide, cx| {
            mutation(&mut tide.status, cx);
            tide.clone()
        });

        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_configuration_defaults_match_empty_toml() {
        let directory = tempfile::tempdir().unwrap();
        let loaded = load_file::<Tide>(&directory.path().join("tide.toml"), Format::Toml).unwrap();
        let empty: Tide = toml::from_str("").unwrap();
        assert_eq!(
            toml::to_string(&loaded.value).unwrap(),
            toml::to_string(&empty).unwrap()
        );
        assert!(loaded.value.show_main_window_on_startup());
        assert!(!loaded.recovered);
    }

    #[test]
    fn restores_configuration_without_exposing_secret_parse_context() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tide.toml");
        let mut config = Tide::default();
        config.set_ai(AiSettings {
            model: "saved-model".into(),
            ..Default::default()
        });
        save_file(&path, &config, Format::Toml).unwrap();
        config.set_locale("zh-CN".into());
        save_file(&path, &config, Format::Toml).unwrap();
        std::fs::write(&path, "[ai]\napi_key = \"test-private-key").unwrap();

        let restored = load_file::<Tide>(&path, Format::Toml).unwrap();
        assert!(restored.recovered);
        assert_eq!(restored.value.ai().model, "saved-model");
        std::fs::write(path.with_extension("toml.bak"), "broken = [").unwrap();
        std::fs::write(&path, "[ai]\napi_key = \"test-private-key").unwrap();
        let error = load_file::<Tide>(&path, Format::Toml).err().unwrap();
        assert!(!format!("{error:#}").contains("test-private-key"));
    }
}
