use std::collections::HashSet;
use std::fs;

use anyhow::Result;
use chrono::{Local, NaiveDate, NaiveTime};
use gpui::{App, AppContext, Context, Entity, Global};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info};

use rust_i18n::t;

use crate::helpers::get_or_create_data_path;

static TASK_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn new_id() -> String {
    let n = TASK_COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{t}{n:04}")
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct DueDate {
    pub date: NaiveDate,
    pub time: Option<NaiveTime>,
}

impl DueDate {
    pub fn new(date: NaiveDate, time: Option<NaiveTime>) -> Self {
        Self { date, time }
    }
}

impl<'de> Deserialize<'de> for DueDate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum DueDateRepr {
            Date(NaiveDate),
            DateTime {
                date: NaiveDate,
                #[serde(default)]
                time: Option<NaiveTime>,
            },
        }

        match DueDateRepr::deserialize(deserializer)? {
            DueDateRepr::Date(date) => Ok(Self::new(date, None)),
            DueDateRepr::DateTime { date, time } => Ok(Self::new(date, time)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: String,
    pub group_id: String,
    pub title: String,
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub due_date: Option<DueDate>,
    #[serde(default)]
    pub is_completed: bool,
    #[serde(default)]
    pub completed_at: Option<NaiveDate>,
    #[serde(default)]
    pub is_starred: bool,
    #[serde(default)]
    pub parent_id: Option<String>,
}

impl Task {
    pub fn new(group_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            group_id: group_id.into(),
            title: title.into(),
            details: None,
            due_date: None,
            is_completed: false,
            completed_at: None,
            is_starred: false,
            parent_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskGroup {
    pub id: String,
    pub name: String,
}

impl TaskGroup {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: new_id(),
            name: name.into(),
        }
    }

    pub fn default_group() -> Self {
        let name: String = t!("sidebar.my_group").into();
        Self {
            id: "my-group".to_string(),
            name,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum SidebarSelection {
    #[default]
    AllTasks,
    Starred,
    Settings,
    Group(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TideData {
    #[serde(default)]
    pub task_groups: Vec<TaskGroup>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub sidebar_selection: SidebarSelection,
    #[serde(skip)]
    settings_return_selection: Option<SidebarSelection>,
}

impl TideData {
    pub fn task_groups(&self) -> &[TaskGroup] {
        &self.task_groups
    }

    pub fn add_task_group(&mut self, group: TaskGroup) {
        self.task_groups.push(group);
    }

    pub fn rename_task_group(&mut self, id: &str, name: String) {
        if let Some(group) = self.task_groups.iter_mut().find(|l| l.id == id) {
            let trimmed = name.trim().to_string();
            if !trimmed.is_empty() {
                group.name = trimmed;
            }
        }
    }

    pub fn reorder_task_group_before(&mut self, from_id: &str, before_id: &str) {
        if from_id == before_id || !self.task_groups.iter().any(|group| group.id == from_id) {
            return;
        }

        // Groups are stored oldest-first but displayed newest-first.
        self.task_groups.reverse();
        let from_pos = self
            .task_groups
            .iter()
            .position(|group| group.id == from_id)
            .expect("group existence checked above");
        let group = self.task_groups.remove(from_pos);
        let to_pos = self
            .task_groups
            .iter()
            .position(|group| group.id == before_id)
            .unwrap_or(self.task_groups.len());
        self.task_groups.insert(to_pos, group);
        self.task_groups.reverse();
    }

    pub fn remove_task_group(&mut self, id: &str) {
        self.task_groups.retain(|l| l.id != id);
        self.tasks.retain(|t| t.group_id != id);
        if self.sidebar_selection == SidebarSelection::Group(id.to_string()) {
            self.sidebar_selection = SidebarSelection::AllTasks;
        }
    }

    pub fn sidebar_selection(&self) -> &SidebarSelection {
        &self.sidebar_selection
    }

    pub fn set_sidebar_selection(&mut self, selection: SidebarSelection) {
        if selection == SidebarSelection::Settings
            && self.sidebar_selection != SidebarSelection::Settings
        {
            self.settings_return_selection = Some(self.sidebar_selection.clone());
        }
        self.sidebar_selection = selection;
    }

    pub fn return_from_settings(&mut self) {
        self.sidebar_selection = self
            .settings_return_selection
            .take()
            .unwrap_or(SidebarSelection::AllTasks);
    }

    pub fn group_id_for_creation(&self) -> Option<String> {
        match &self.sidebar_selection {
            SidebarSelection::Group(id) => Some(id.clone()),
            _ => None,
        }
    }

    pub fn visible_tasks(&self) -> Vec<&Task> {
        match &self.sidebar_selection {
            SidebarSelection::AllTasks => self
                .tasks
                .iter()
                .filter(|t| t.parent_id.is_none())
                .collect(),
            SidebarSelection::Starred => self.tasks.iter().filter(|t| t.is_starred).collect(),
            SidebarSelection::Settings => Vec::new(),
            SidebarSelection::Group(id) => self
                .tasks
                .iter()
                .filter(|t| t.parent_id.is_none() && &t.group_id == id)
                .collect(),
        }
    }

    pub fn subtasks_of(&self, parent_id: &str) -> Vec<&Task> {
        self.tasks
            .iter()
            .filter(|t| t.parent_id.as_deref() == Some(parent_id))
            .collect()
    }

    pub fn insert_task(&mut self, index: usize, task: Task) {
        let top_positions: Vec<usize> = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.parent_id.is_none())
            .map(|(i, _)| i)
            .collect();
        let flat_idx = top_positions
            .get(index)
            .copied()
            .unwrap_or(self.tasks.len());
        self.tasks.insert(flat_idx, task);
    }

    pub fn update_task(&mut self, task: Task) {
        if let Some(idx) = self.tasks.iter().position(|t| t.id == task.id) {
            self.tasks[idx] = task;
        }
    }

    pub fn reorder_task_before(&mut self, from_id: &str, before_id: &str) {
        if from_id == before_id {
            return;
        }
        let Some(from_pos) = self.tasks.iter().position(|t| t.id == from_id) else {
            return;
        };
        let task = self.tasks.remove(from_pos);
        let to_pos = self
            .tasks
            .iter()
            .position(|t| t.id == before_id)
            .unwrap_or(self.tasks.len());
        self.tasks.insert(to_pos, task);
    }

    pub fn remove_task(&mut self, task_id: &str) {
        self.tasks
            .retain(|t| t.id != task_id && t.parent_id.as_deref() != Some(task_id));
    }

    pub fn remove_completed_tasks(&mut self, task_ids: &[String]) {
        let task_ids = task_ids.iter().map(String::as_str).collect::<HashSet<_>>();
        self.tasks.retain(|task| {
            !task_ids.contains(task.id.as_str())
                && !task
                    .parent_id
                    .as_deref()
                    .is_some_and(|parent_id| task_ids.contains(parent_id))
        });
    }

    pub fn toggle_task_completion(&mut self, task_id: &str) {
        let Some(current) = self.tasks.iter().find(|t| t.id == task_id) else {
            return;
        };

        let next_completed = !current.is_completed;
        let next_completed_at = if next_completed {
            Some(Local::now().date_naive())
        } else {
            None
        };

        for task in self
            .tasks
            .iter_mut()
            .filter(|t| t.id == task_id || t.parent_id.as_deref() == Some(task_id))
        {
            task.is_completed = next_completed;
            task.completed_at = next_completed_at;
        }
    }

    pub fn toggle_task_star(&mut self, task_id: &str) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
            task.is_starred = !task.is_starred;
        }
    }

    pub fn set_task_due_date(&mut self, task_id: &str, due: Option<DueDate>) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
            task.due_date = due;
        }
    }

    pub fn insert_subtask(&mut self, parent_id: &str, index: usize, mut subtask: Task) {
        let Some(parent) = self.tasks.iter().find(|t| t.id == parent_id) else {
            return;
        };
        subtask.group_id = parent.group_id.clone();
        subtask.parent_id = Some(parent_id.to_string());

        let parent_pos = match self.tasks.iter().position(|t| t.id == parent_id) {
            Some(p) => p,
            None => return,
        };
        let sibling_positions: Vec<usize> = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.parent_id.as_deref() == Some(parent_id))
            .map(|(i, _)| i)
            .collect();
        let flat_idx = sibling_positions.get(index).copied().unwrap_or_else(|| {
            sibling_positions
                .last()
                .map(|p| p + 1)
                .unwrap_or(parent_pos + 1)
        });
        self.tasks.insert(flat_idx, subtask);
    }

    pub fn set_subtask_text(&mut self, sub_id: &str, title: String, details: Option<String>) {
        if let Some(sub) = self.tasks.iter_mut().find(|t| t.id == sub_id) {
            sub.title = title;
            sub.details = details;
        }
    }

    pub fn remove_subtask(&mut self, subtask_id: &str) {
        self.remove_task(subtask_id);
    }

    pub fn reorder_subtask_before(&mut self, from_id: &str, before_id: &str) {
        self.reorder_task_before(from_id, before_id);
    }

    pub fn promote_subtask_to_task(&mut self, subtask_id: &str, before_id: &str) {
        if let Some(sub) = self.tasks.iter_mut().find(|t| t.id == subtask_id) {
            if sub.parent_id.is_none() {
                return;
            }
            sub.parent_id = None;
        } else {
            return;
        }
        self.reorder_task_before(subtask_id, before_id);
    }

    pub fn demote_task_to_subtask(&mut self, task_id: &str, parent_id: &str, before_id: &str) {
        if task_id == parent_id
            || self
                .tasks
                .iter()
                .any(|task| task.parent_id.as_deref() == Some(task_id))
        {
            return;
        }

        let Some(parent) = self
            .tasks
            .iter()
            .find(|task| task.id == parent_id && task.parent_id.is_none())
        else {
            return;
        };
        let parent_group_id = parent.group_id.clone();

        let Some(from_pos) = self
            .tasks
            .iter()
            .position(|task| task.id == task_id && task.parent_id.is_none())
        else {
            return;
        };
        let mut task = self.tasks.remove(from_pos);
        task.group_id = parent_group_id;
        task.parent_id = Some(parent_id.to_string());

        let insert_pos = self
            .tasks
            .iter()
            .position(|task| task.id == before_id && task.parent_id.as_deref() == Some(parent_id))
            .or_else(|| {
                self.tasks
                    .iter()
                    .enumerate()
                    .filter(|(_, task)| task.parent_id.as_deref() == Some(parent_id))
                    .map(|(index, _)| index + 1)
                    .last()
            })
            .or_else(|| {
                self.tasks
                    .iter()
                    .position(|task| task.id == parent_id)
                    .map(|index| index + 1)
            })
            .unwrap_or(self.tasks.len());

        self.tasks.insert(insert_pos, task);
    }
}

pub fn save_data(data: &TideData) -> Result<()> {
    let path = get_or_create_data_path()?;
    let value = serde_json::to_string_pretty(data)?;
    fs::write(path, value)?;
    Ok(())
}

pub fn load_data() -> Result<TideData> {
    let path = get_or_create_data_path()?;
    let value = fs::read_to_string(path)?;
    let mut data: TideData = serde_json::from_str(&value)?;
    if data.task_groups.is_empty() {
        data.task_groups.push(TaskGroup::default_group());
    }
    Ok(data)
}

#[derive(Debug, Clone)]
pub struct TideDataStore {
    entity: Entity<TideData>,
}

impl TideDataStore {
    pub fn new(entity: Entity<TideData>) -> Self {
        Self { entity }
    }

    pub fn read<'a>(&self, cx: &'a App) -> &'a TideData {
        self.entity.read(cx)
    }

    pub fn update<R, C: AppContext>(
        &self,
        cx: &mut C,
        f: impl FnOnce(&mut TideData, &mut Context<TideData>) -> R,
    ) -> R {
        self.entity.update(cx, f)
    }
}

impl Global for TideDataStore {}

#[inline]
pub fn update_data_and_save<F>(cx: &App, action_name: &'static str, mutation: F)
where
    F: FnOnce(&mut TideData, &App) + Send + 'static + Clone,
{
    let store = cx.global::<TideDataStore>().clone();

    cx.spawn(async move |cx| {
        let current = store.update(cx, |data, cx| {
            mutation(data, cx);
            cx.notify();
            data.clone()
        });

        cx.background_executor()
            .spawn(async move {
                if let Err(e) = save_data(&current) {
                    error!(error = %e, action = action_name, "Failed to save tasks");
                } else {
                    info!(action = action_name, "Tasks saved successfully");
                }
            })
            .await;

        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_tasks_saved_before_due_time_was_added() {
        let value = r#"
        {
          "task_groups": [
            { "id": "my-group", "name": "My Group" }
          ],
          "tasks": [
            {
              "id": "task-1",
              "group_id": "my-group",
              "title": "Old dated task",
              "due_date": "2026-05-20",
              "is_completed": false,
              "is_starred": true
            },
            {
              "id": "task-2",
              "group_id": "my-group",
              "title": "Older minimal task"
            }
          ],
          "sidebar_selection": "AllTasks"
        }
        "#;

        let data: TideData = serde_json::from_str(value).expect("old data should load");

        assert_eq!(data.tasks.len(), 2);
        assert_eq!(
            data.tasks[0].due_date,
            Some(DueDate::new(
                NaiveDate::from_ymd_opt(2026, 5, 20).unwrap(),
                None
            ))
        );
        assert_eq!(data.tasks[1].details, None);
        assert_eq!(data.tasks[1].due_date, None);
        assert!(!data.tasks[1].is_completed);
        assert_eq!(data.tasks[1].completed_at, None);
        assert!(!data.tasks[1].is_starred);
        assert_eq!(data.tasks[1].parent_id, None);
    }

    #[test]
    fn removes_completed_tasks_and_their_children() {
        let mut parent = Task::new("my-group", "Done parent");
        parent.id = "parent".to_string();
        parent.is_completed = true;

        let mut child = Task::new("my-group", "Child");
        child.id = "child".to_string();
        child.parent_id = Some(parent.id.clone());

        let mut other = Task::new("my-group", "Other");
        other.id = "other".to_string();

        let mut data = TideData {
            tasks: vec![parent, child, other],
            ..Default::default()
        };

        data.remove_completed_tasks(&["parent".to_string()]);

        assert_eq!(data.tasks.len(), 1);
        assert_eq!(data.tasks[0].id, "other");
    }

    #[test]
    fn reorders_task_groups_in_display_order() {
        let group = |id: &str| TaskGroup {
            id: id.to_string(),
            name: id.to_string(),
        };
        let mut data = TideData {
            task_groups: vec![group("oldest"), group("middle"), group("newest")],
            ..Default::default()
        };

        data.reorder_task_group_before("oldest", "newest");
        assert_eq!(
            data.task_groups
                .iter()
                .rev()
                .map(|group| group.id.as_str())
                .collect::<Vec<_>>(),
            vec!["oldest", "newest", "middle"]
        );

        data.reorder_task_group_before("oldest", "");
        assert_eq!(
            data.task_groups
                .iter()
                .rev()
                .map(|group| group.id.as_str())
                .collect::<Vec<_>>(),
            vec!["newest", "middle", "oldest"]
        );
    }

    #[test]
    fn demotes_top_level_task_back_to_subtask() {
        let mut parent = Task::new("group", "Parent");
        parent.id = "parent".to_string();
        let mut sibling = Task::new("group", "Sibling");
        sibling.id = "sibling".to_string();
        sibling.parent_id = Some(parent.id.clone());
        let mut promoted = Task::new("group", "Promoted");
        promoted.id = "promoted".to_string();

        let mut data = TideData {
            tasks: vec![parent, sibling, promoted],
            ..Default::default()
        };

        data.demote_task_to_subtask("promoted", "parent", "sibling");

        let promoted = data
            .tasks
            .iter()
            .find(|task| task.id == "promoted")
            .unwrap();
        assert_eq!(promoted.parent_id.as_deref(), Some("parent"));
        assert_eq!(
            data.subtasks_of("parent")
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec!["promoted", "sibling"]
        );
    }

    #[test]
    fn does_not_demote_task_that_has_subtasks() {
        let mut parent = Task::new("group", "Parent");
        parent.id = "parent".to_string();
        let mut task = Task::new("group", "Task");
        task.id = "task".to_string();
        let mut child = Task::new("group", "Child");
        child.id = "child".to_string();
        child.parent_id = Some(task.id.clone());

        let mut data = TideData {
            tasks: vec![parent, task, child],
            ..Default::default()
        };

        data.demote_task_to_subtask("task", "parent", "");

        assert_eq!(
            data.tasks
                .iter()
                .find(|task| task.id == "task")
                .unwrap()
                .parent_id,
            None
        );
    }

    #[test]
    fn returns_to_previous_view_after_settings() {
        let mut data = TideData::default();
        let group = SidebarSelection::Group("work".to_string());
        data.set_sidebar_selection(group.clone());
        data.set_sidebar_selection(SidebarSelection::Settings);

        data.return_from_settings();

        assert_eq!(data.sidebar_selection(), &group);
    }
}
