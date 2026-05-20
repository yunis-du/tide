use std::{collections::HashMap, fs, time::Duration};

#[cfg(target_os = "macos")]
use std::process::Command;

use anyhow::Result;
use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeZone};
use gpui::{App, AsyncApp};
use rust_i18n::t;
use serde::{Deserialize, Serialize};
use tracing::{error, warn};

use crate::{
    helpers::get_or_create_notifications_path,
    state::{NotificationSettings, Task, TideDataStore, TideStore},
};

const MACOS_BUNDLE_IDENTIFIER: &str = "com.yunisdu.tide";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct NotificationRecords {
    records: HashMap<String, TaskNotificationRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TaskNotificationRecord {
    due_key: String,
    fired_kinds: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NotificationEvent {
    task_id: String,
    due_key: String,
    kind: String,
    title: String,
    body: String,
}

pub fn spawn(cx: &mut App) {
    init_platform_notifications();

    cx.spawn(async move |cx: &mut AsyncApp| {
        let mut records = cx
            .background_executor()
            .spawn(async { load_records().unwrap_or_default() })
            .await;

        loop {
            let (events, active_due_keys) = cx.update(|cx| {
                let config = cx.global::<TideStore>().read(cx);
                let settings = config.notifications().clone();
                let locale = config.locale().to_string();
                let data = cx.global::<TideDataStore>().read(cx);
                (
                    pending_notifications(
                        &data.tasks,
                        &records,
                        Local::now(),
                        &settings,
                        locale.as_str(),
                    ),
                    active_due_keys(&data.tasks),
                )
            });

            let mut changed = clean_records(&mut records, &active_due_keys);
            if !events.is_empty() {
                for event in events {
                    match show_event(&event) {
                        Ok(()) => {
                            mark_fired(&mut records, &event);
                            changed = true;
                        }
                        Err(err) => {
                            warn!(
                                error = %err,
                                task_id = %event.task_id,
                                kind = %event.kind,
                                "failed to show task notification"
                            );
                        }
                    }
                }
            }

            if changed {
                let snapshot = records.clone();
                cx.background_executor()
                    .spawn(async move {
                        if let Err(err) = save_records(&snapshot) {
                            error!(error = %err, "failed to save notification records");
                        }
                    })
                    .detach();
            }

            cx.background_executor()
                .timer(Duration::from_secs(60))
                .await;
        }
    })
    .detach();
}

fn init_platform_notifications() {
    #[cfg(target_os = "macos")]
    {
        if let Err(err) = notify_rust::set_application(MACOS_BUNDLE_IDENTIFIER) {
            warn!(
                error = %err,
                bundle_identifier = MACOS_BUNDLE_IDENTIFIER,
                "failed to initialize macOS notifications"
            );
        }
    }
}

fn active_due_keys(tasks: &[Task]) -> HashMap<String, String> {
    tasks
        .iter()
        .filter_map(|task| {
            if task.is_completed {
                return None;
            }
            task.due_date
                .map(|due| (task.id.clone(), due_key(due.date, due.time)))
        })
        .collect()
}

fn clean_records(
    records: &mut NotificationRecords,
    active_due_keys: &HashMap<String, String>,
) -> bool {
    let before = records.records.len();
    records.records.retain(|task_id, record| {
        active_due_keys
            .get(task_id)
            .is_some_and(|due_key| due_key == &record.due_key)
    });
    before != records.records.len()
}

fn pending_notifications(
    tasks: &[Task],
    records: &NotificationRecords,
    now: DateTime<Local>,
    settings: &NotificationSettings,
    locale: &str,
) -> Vec<NotificationEvent> {
    if !settings.enabled {
        return Vec::new();
    }

    let titles_by_id: HashMap<&str, &str> = tasks
        .iter()
        .map(|task| (task.id.as_str(), task.title.as_str()))
        .collect();

    let mut events = Vec::new();
    for task in tasks
        .iter()
        .filter(|task| !task.is_completed && task.due_date.is_some())
    {
        let due = task.due_date.expect("filtered due date");
        let due_key = due_key(due.date, due.time);
        let record = records.records.get(task.id.as_str());
        let task_title = notification_task_title(task, &titles_by_id);

        if let Some(time) = due.time {
            let due_at = local_datetime(due.date, time);
            for minutes in &settings.before_due_minutes {
                if *minutes <= 0 {
                    continue;
                }
                let kind = format!("before_{minutes}m");
                let fire_at = due_at - chrono::Duration::minutes(*minutes);
                if now >= fire_at && now < due_at && !has_fired(record, &due_key, &kind) {
                    events.push(NotificationEvent {
                        task_id: task.id.clone(),
                        due_key: due_key.clone(),
                        kind,
                        title: t!("notification.before_due_title", locale = locale).into(),
                        body: t!(
                            "notification.before_due_body",
                            task = task_title,
                            minutes = minutes.to_string(),
                            locale = locale
                        )
                        .into(),
                    });
                }
            }

            if now >= due_at && !has_fired(record, &due_key, "due") {
                events.push(NotificationEvent {
                    task_id: task.id.clone(),
                    due_key: due_key.clone(),
                    kind: "due".to_string(),
                    title: t!("notification.due_title", locale = locale).into(),
                    body: t!("notification.due_body", task = task_title, locale = locale).into(),
                });
            }
        } else if due.date == now.date_naive() {
            let fire_at = local_datetime(due.date, settings.default_no_time_reminder);
            if now >= fire_at && !has_fired(record, &due_key, "no_time_today") {
                events.push(NotificationEvent {
                    task_id: task.id.clone(),
                    due_key: due_key.clone(),
                    kind: "no_time_today".to_string(),
                    title: t!("notification.today_title", locale = locale).into(),
                    body: t!(
                        "notification.today_body",
                        task = task_title,
                        locale = locale
                    )
                    .into(),
                });
            }
        }

        if due.date < now.date_naive() {
            let kind = format!("overdue_daily:{}", now.date_naive());
            let fire_at = local_datetime(now.date_naive(), settings.overdue_daily_time);
            if now >= fire_at && !has_fired(record, &due_key, &kind) {
                events.push(NotificationEvent {
                    task_id: task.id.clone(),
                    due_key,
                    kind,
                    title: t!("notification.overdue_title", locale = locale).into(),
                    body: t!(
                        "notification.overdue_body",
                        task = task_title,
                        locale = locale
                    )
                    .into(),
                });
            }
        }
    }

    events
}

fn notification_task_title(task: &Task, titles_by_id: &HashMap<&str, &str>) -> String {
    match task
        .parent_id
        .as_deref()
        .and_then(|parent_id| titles_by_id.get(parent_id).copied())
    {
        Some(parent_title) => format!("{parent_title} / {}", task.title),
        None => task.title.clone(),
    }
}

fn has_fired(record: Option<&TaskNotificationRecord>, due_key: &str, kind: &str) -> bool {
    record.is_some_and(|record| {
        record.due_key == due_key && record.fired_kinds.iter().any(|fired| fired == kind)
    })
}

fn mark_fired(records: &mut NotificationRecords, event: &NotificationEvent) {
    let record = records
        .records
        .entry(event.task_id.clone())
        .or_insert_with(|| TaskNotificationRecord {
            due_key: event.due_key.clone(),
            fired_kinds: Vec::new(),
        });

    if record.due_key != event.due_key {
        record.due_key = event.due_key.clone();
        record.fired_kinds.clear();
    }

    if !record
        .fired_kinds
        .iter()
        .any(|kind| kind == event.kind.as_str())
    {
        record.fired_kinds.push(event.kind.clone());
    }
}

fn due_key(date: NaiveDate, time: Option<NaiveTime>) -> String {
    match time {
        Some(time) => format!("{date}T{time}"),
        None => date.to_string(),
    }
}

fn local_datetime(date: NaiveDate, time: NaiveTime) -> DateTime<Local> {
    let naive = date.and_time(time);
    Local
        .from_local_datetime(&naive)
        .earliest()
        .unwrap_or_else(|| Local.from_utc_datetime(&naive))
}

fn load_records() -> Result<NotificationRecords> {
    let path = get_or_create_notifications_path()?;
    let value = fs::read_to_string(path)?;
    if value.trim().is_empty() {
        return Ok(NotificationRecords::default());
    }
    Ok(serde_json::from_str(&value)?)
}

fn save_records(records: &NotificationRecords) -> Result<()> {
    let path = get_or_create_notifications_path()?;
    fs::write(path, serde_json::to_string_pretty(records)?)?;
    Ok(())
}

fn show_event(event: &NotificationEvent) -> Result<()> {
    let result = notify_rust::Notification::new()
        .summary(&event.title)
        .body(&event.body)
        .appname("Tide")
        .show();

    match result {
        Ok(_) => Ok(()),
        Err(err) => {
            #[cfg(target_os = "macos")]
            {
                warn!(
                    error = %err,
                    "native macOS notification failed, trying osascript fallback"
                );
                return show_macos_fallback(event);
            }

            #[cfg(not(target_os = "macos"))]
            {
                Err(err.into())
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn show_macos_fallback(event: &NotificationEvent) -> Result<()> {
    let script = format!(
        "display notification {} with title {}",
        applescript_string(&event.body),
        applescript_string(&event.title)
    );
    let status = Command::new("osascript").arg("-e").arg(script).status()?;
    if !status.success() {
        anyhow::bail!("osascript notification fallback failed with status {status}");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn applescript_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DueDate;

    fn local_at(date: NaiveDate, time: NaiveTime) -> DateTime<Local> {
        local_datetime(date, time)
    }

    fn task(id: &str, title: &str, due: DueDate) -> Task {
        Task {
            id: id.to_string(),
            group_id: "group".to_string(),
            title: title.to_string(),
            details: None,
            due_date: Some(due),
            is_completed: false,
            completed_at: None,
            is_starred: false,
            parent_id: None,
        }
    }

    #[test]
    fn fires_before_due_once() {
        let date = NaiveDate::from_ymd_opt(2026, 5, 20).unwrap();
        let due_time = NaiveTime::from_hms_opt(10, 0, 0).unwrap();
        let now = local_at(date, NaiveTime::from_hms_opt(9, 45, 0).unwrap());
        let settings = NotificationSettings::default();
        let tasks = vec![task("1", "Ship it", DueDate::new(date, Some(due_time)))];

        let events = pending_notifications(
            &tasks,
            &NotificationRecords::default(),
            now,
            &settings,
            "en",
        );

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "before_15m");
    }

    #[test]
    fn skips_completed_tasks() {
        let date = NaiveDate::from_ymd_opt(2026, 5, 20).unwrap();
        let mut task = task("1", "Done", DueDate::new(date, None));
        task.is_completed = true;
        let now = local_at(date, NaiveTime::from_hms_opt(10, 0, 0).unwrap());

        let events = pending_notifications(
            &[task],
            &NotificationRecords::default(),
            now,
            &NotificationSettings::default(),
            "en",
        );

        assert!(events.is_empty());
    }
}
