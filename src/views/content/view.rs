use gpui::{Context, Entity, FocusHandle, ScrollHandle, Subscription, Window, prelude::*};
use gpui_component::{
    calendar::{CalendarEvent, CalendarState, Date},
    input::{InputEvent, InputState},
};

use crate::{
    helpers::{due_time_label, parse_due_time},
    state::{DueDate, TideStore, update_data_and_save},
};

pub struct TaskView {
    pub(super) title_input: Entity<InputState>,
    pub(super) details_input: Entity<InputState>,
    pub(super) time_input: Entity<InputState>,
    pub(super) calendar_state: Entity<CalendarState>,
    pub(super) pending_due_date: Option<DueDate>,

    pub(super) due_picker_calendar_state: Entity<CalendarState>,
    pub(super) due_picker_time_input: Entity<InputState>,
    pub(super) due_picker_for: Option<String>,
    pub(super) focus_handle: FocusHandle,
    pub(super) pending_scroll_handle: ScrollHandle,

    pub(super) batch_count: usize,
    pub(super) subtask_batch_count: usize,
    pub(super) completed_expanded: bool,
    pub(super) hovered_task_id: Option<String>,
    pub(super) selected_task_id: Option<String>,
    pub(super) dragging_task_id: Option<String>,
    pub(super) hovered_subtask_id: Option<String>,
    pub(super) selected_subtask_id: Option<String>,
    pub(super) dragging_subtask_id: Option<String>,

    _subs: Vec<Subscription>,
}

impl TaskView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let title_input = cx.new(|cx| InputState::new(window, cx));
        let details_input = cx.new(|cx| InputState::new(window, cx).auto_grow(1, 5));
        let time_input = cx.new(|cx| InputState::new(window, cx));
        let calendar_state = cx.new(|cx| CalendarState::new(window, cx));
        let due_picker_calendar_state = cx.new(|cx| CalendarState::new(window, cx));
        let due_picker_time_input = cx.new(|cx| InputState::new(window, cx));

        let mut subs = Vec::new();

        subs.push(cx.subscribe_in(
            &title_input,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let status = cx.global::<TideStore>().read(cx).status();
                    let is_subtask_mode =
                        status.adding_subtask_for().is_some() || status.edit_subtask_id().is_some();
                    if is_subtask_mode {
                        Self::enter_subtask(this, window, cx);
                    } else {
                        Self::enter_task(this, window, cx);
                    }
                }
                _ => {}
            },
        ));

        subs.push(cx.subscribe_in(
            &calendar_state,
            window,
            |this: &mut Self, _, event: &CalendarEvent, _window, cx| match event {
                CalendarEvent::Selected(date) => {
                    if let Some(picked) = date.start() {
                        let time_value = this.time_input.read(cx).value().to_string();
                        this.pending_due_date =
                            Some(DueDate::new(picked, parse_due_time(&time_value)));
                        cx.notify();
                    }
                }
            },
        ));

        subs.push(cx.subscribe_in(
            &time_input,
            window,
            |this: &mut Self, _, event: &InputEvent, _window, cx| match event {
                InputEvent::Change => {
                    let Some(mut due) = this.pending_due_date else {
                        return;
                    };
                    let value = this.time_input.read(cx).value().to_string();
                    due.time = parse_due_time(&value);
                    this.pending_due_date = Some(due);
                    cx.notify();
                }
                _ => {}
            },
        ));

        subs.push(cx.subscribe_in(
            &due_picker_calendar_state,
            window,
            |this: &mut Self, _, event: &CalendarEvent, window, cx| match event {
                CalendarEvent::Selected(date) => {
                    if let Some(picked) = date.start() {
                        if let Some(id) = this.due_picker_for.clone() {
                            let time_value =
                                this.due_picker_time_input.read(cx).value().to_string();
                            let due = DueDate::new(picked, parse_due_time(&time_value));
                            update_data_and_save(cx, "set_task_due_date", move |data, _| {
                                data.set_task_due_date(&id, Some(due));
                            });
                            this.due_picker_calendar_state.update(cx, |state, cx| {
                                state.set_date(Date::Single(Some(picked)), window, cx);
                            });
                            cx.notify();
                        }
                    }
                }
            },
        ));

        subs.push(cx.subscribe_in(
            &due_picker_time_input,
            window,
            |this: &mut Self, _, event: &InputEvent, _window, cx| match event {
                InputEvent::Change => {
                    let Some(id) = this.due_picker_for.clone() else {
                        return;
                    };
                    let value = this.due_picker_time_input.read(cx).value().to_string();
                    let time = parse_due_time(&value);
                    update_data_and_save(cx, "set_task_due_time", move |data, _| {
                        if let Some(task) = data.tasks.iter_mut().find(|task| task.id == id)
                            && let Some(mut due) = task.due_date
                        {
                            due.time = time;
                            task.due_date = Some(due);
                        }
                    });
                    cx.notify();
                }
                _ => {}
            },
        ));

        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                Self::close_form(this, window, cx);
            }
        })
        .detach();

        let completed_expanded = cx
            .global::<TideStore>()
            .read(cx)
            .completed_expanded_by_default();

        Self {
            title_input,
            details_input,
            time_input,
            calendar_state,
            pending_due_date: None,
            due_picker_calendar_state,
            due_picker_time_input,
            due_picker_for: None,
            focus_handle: cx.focus_handle(),
            pending_scroll_handle: ScrollHandle::new(),
            batch_count: 0,
            subtask_batch_count: 0,
            completed_expanded,
            hovered_task_id: None,
            selected_task_id: None,
            dragging_task_id: None,
            hovered_subtask_id: None,
            selected_subtask_id: None,
            dragging_subtask_id: None,
            _subs: subs,
        }
    }

    pub(super) fn reset_pending_due_date(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_due_date = None;
        self.calendar_state.update(cx, |state, cx| {
            state.set_date(Date::Single(None), window, cx);
        });
        self.time_input.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
    }

    pub(super) fn set_pending_due_date(
        &mut self,
        due: Option<DueDate>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_due_date = due;
        self.calendar_state.update(cx, |state, cx| {
            let d = match due {
                Some(d) => Date::Single(Some(d.date)),
                None => Date::Single(None),
            };
            state.set_date(d, window, cx);
        });
        self.time_input.update(cx, |state, cx| {
            let value = due
                .and_then(|d| d.time)
                .map(due_time_label)
                .unwrap_or_default();
            state.set_value(value, window, cx);
        });
        cx.notify();
    }

    pub(super) fn details_preview(details: &str) -> String {
        let mut lines = details.lines();
        let first = lines.next();
        let second = lines.next();
        let has_more = lines.next().is_some();

        match (first, second, has_more) {
            (Some(l1), Some(l2), true) => format!("{l1}\n{l2}..."),
            _ => details.to_string(),
        }
    }
}
