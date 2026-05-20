use chrono::{Datelike, Local, NaiveTime, Timelike, Weekday};
use gpui::{App, Hsla};
use gpui_component::ActiveTheme;
use rust_i18n::t;

use crate::state::DueDate;

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

pub fn due_date_label(cx: &App, due: DueDate) -> String {
    let date = due.date;
    let today = Local::now().date_naive();
    let delta = (date - today).num_days();
    let l = locale(cx);
    let label: String = match delta {
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
    };

    match due.time {
        Some(time) => format!("{label} {}", due_time_label(time)),
        None => label,
    }
}

pub fn due_time_label(time: NaiveTime) -> String {
    format!("{:02}:{:02}", time.hour(), time.minute())
}

pub fn parse_due_time(value: &str) -> Option<NaiveTime> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    NaiveTime::parse_from_str(trimmed, "%H:%M").ok()
}

pub fn due_date_color(cx: &App, due: DueDate) -> Hsla {
    let today = Local::now().date_naive();
    let date = due.date;
    if date < today {
        cx.theme().danger
    } else if date == today {
        cx.theme().info_active
    } else {
        cx.theme().muted_foreground
    }
}
