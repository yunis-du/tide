use chrono::{Datelike, Local, NaiveDate, Weekday};
use gpui::{App, Hsla};
use gpui_component::ActiveTheme;
use rust_i18n::t;

use super::{i18n_content, locale};

pub fn weekday_label(wd: Weekday, locale: &str) -> String {
    let key = match wd {
        Weekday::Mon => "content.weekday_mon",
        Weekday::Tue => "content.weekday_tue",
        Weekday::Wed => "content.weekday_wed",
        Weekday::Thu => "content.weekday_thu",
        Weekday::Fri => "content.weekday_fri",
        Weekday::Sat => "content.weekday_sat",
        Weekday::Sun => "content.weekday_sun",
    };
    t!(key, locale = locale).into()
}

pub fn due_date_label(cx: &App, date: NaiveDate) -> String {
    let today = Local::now().date_naive();
    let delta = (date - today).num_days();
    let l = locale(cx);
    match delta {
        0 => i18n_content(cx, "today"),
        1 => i18n_content(cx, "tomorrow"),
        -1 => i18n_content(cx, "yesterday"),
        n if n < -1 => t!(
            "content.days_ago",
            days = (-n).to_string(),
            locale = l.as_str()
        )
        .into(),
        _ => t!(
            "content.due_date_full",
            month = date.month(),
            day = date.day(),
            weekday = weekday_label(date.weekday(), l.as_str()),
            locale = l.as_str()
        )
        .into(),
    }
}

pub fn due_date_color(cx: &App, date: NaiveDate) -> Hsla {
    let today = Local::now().date_naive();
    if date < today {
        cx.theme().danger
    } else if date == today {
        cx.theme().info_active
    } else {
        cx.theme().muted_foreground
    }
}
