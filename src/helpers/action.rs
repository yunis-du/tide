use gpui::{Action, KeyBinding};
use schemars::JsonSchema;
use serde::Deserialize;

/// Theme selection actions for the settings menu
#[derive(Clone, Copy, PartialEq, Debug, Deserialize, JsonSchema, Action)]
pub enum ThemeAction {
    /// Light theme mode
    Light,
    /// Dark theme mode
    Dark,
    /// Follow system theme
    System,
}

/// Locale/language selection actions for the settings menu
#[derive(Clone, Copy, PartialEq, Debug, Deserialize, JsonSchema, Action)]
pub enum LocaleAction {
    /// English language
    En,
    /// Chinese Simplified language
    ZhCN,
}

#[derive(Clone, Copy, PartialEq, Debug, Deserialize, JsonSchema, Action)]
pub enum HotKeyAction {
    /// Quit the application.
    Quit,
    /// Hide the window into the system tray.
    Hide,
}

pub fn new_hot_keys() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("cmd-q", HotKeyAction::Quit, None),
        KeyBinding::new("cmd-w", HotKeyAction::Hide, None),
    ]
}
