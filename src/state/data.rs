use std::fs;

use anyhow::Result;
use chrono::{Local, NaiveDate};
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Task {
    pub id: String,
    pub group_id: String,
    pub title: String,
    pub details: Option<String>,
    pub due_date: Option<NaiveDate>,
    pub is_completed: bool,
    pub completed_at: Option<NaiveDate>,
    pub is_starred: bool,
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
    Group(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TideData {
    pub task_groups: Vec<TaskGroup>,
    pub tasks: Vec<Task>,
    pub sidebar_selection: SidebarSelection,
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
        self.sidebar_selection = selection;
    }

    pub fn default_group_id_for_creation(&self) -> String {
        match &self.sidebar_selection {
            SidebarSelection::Group(id) => id.clone(),
            _ => self
                .task_groups
                .first()
                .map(|l| l.id.clone())
                .unwrap_or_else(|| "my-tasks".to_string()),
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

    pub fn set_task_due_date(&mut self, task_id: &str, date: Option<NaiveDate>) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
            task.due_date = date;
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
    ) -> C::Result<R> {
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

        if let Ok(data) = current {
            cx.background_executor()
                .spawn(async move {
                    if let Err(e) = save_data(&data) {
                        error!(error = %e, action = action_name, "Failed to save tasks");
                    } else {
                        info!(action = action_name, "Tasks saved successfully");
                    }
                })
                .await;
        }

        cx.update(|cx| cx.refresh_windows()).ok();
    })
    .detach();
}
