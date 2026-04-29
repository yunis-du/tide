use anyhow::Result;
use gpui::{App, AppContext, Bounds, Context, Entity, Global, Pixels};
use gpui_component::ThemeMode;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::helpers::get_or_create_config_path;

const LIGHT_THEME_MODE: &str = "light";
const DARK_THEME_MODE: &str = "dark";

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    #[serde(skip)]
    status: TideStatus,
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
}

pub fn save_config(config: &Tide) -> Result<()> {
    let path = get_or_create_config_path()?;
    let value = toml::to_string(config)?;
    std::fs::write(path, value)?;
    Ok(())
}

pub fn load_config() -> Result<Tide> {
    let path = get_or_create_config_path()?;
    let value = std::fs::read_to_string(path)?;
    let config: Tide = toml::from_str(&value)?;
    Ok(config)
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
    ) -> C::Result<R> {
        self.entity.update(cx, f)
    }
}

impl Global for TideStore {}

#[inline]
pub fn update_and_save<F>(cx: &App, action_name: &'static str, mutation: F)
where
    F: FnOnce(&mut Tide, &App) + Send + 'static + Clone,
{
    let store = cx.global::<TideStore>().clone();

    cx.spawn(async move |cx| {
        let current = store.update(cx, |tide, cx| {
            mutation(tide, cx);
            tide.clone()
        });

        if let Ok(tide) = current {
            cx.background_executor()
                .spawn(async move {
                    if let Err(e) = save_config(&tide) {
                        error!(error = %e, action = action_name, "Failed to save config");
                    } else {
                        info!(action = action_name, "Config saved successfully");
                    }
                })
                .await;
        }

        cx.update(|cx| cx.refresh_windows()).ok();
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

        cx.update(|cx| cx.refresh_windows()).ok();
    })
    .detach();
}
