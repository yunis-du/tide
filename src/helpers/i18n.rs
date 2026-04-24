use gpui::App;
use locale_config::Locale;
use rust_i18n::t;

use crate::state::TideStore;

pub fn default_ui_locale() -> &'static str {
    let raw = Locale::current().to_string();
    let primary = raw
        .split_once('-')
        .or_else(|| raw.split_once('_'))
        .map(|(lang, _)| lang)
        .unwrap_or(raw.as_str());
    if primary.eq_ignore_ascii_case("zh") {
        "zh-CN"
    } else {
        "en"
    }
}

pub fn locale(cx: &App) -> String {
    cx.global::<TideStore>().read(cx).locale().to_string()
}

pub fn i18n_titlebar(cx: &App, key: &str) -> String {
    let l = locale(cx);
    t!(format!("titlebar.{key}"), locale = l).into()
}

pub fn i18n_sidebar(cx: &App, key: &str) -> String {
    let l = locale(cx);
    t!(format!("sidebar.{key}"), locale = l).into()
}

pub fn i18n_content(cx: &App, key: &str) -> String {
    let l = locale(cx);
    t!(format!("content.{key}"), locale = l).into()
}

pub fn i18n_tray(cx: &App, key: &str) -> String {
    let l = locale(cx);
    t!(format!("tray.{key}"), locale = l).into()
}

pub fn i18n_updater(cx: &App, key: &str) -> String {
    let l = locale(cx);
    t!(format!("updater.{key}"), locale = l).into()
}

pub fn i18n_tray_tooltip_count(cx: &App, count: usize) -> String {
    let l = locale(cx);
    t!("tray.tooltip_count", count = count.to_string(), locale = l.as_str()).into()
}
