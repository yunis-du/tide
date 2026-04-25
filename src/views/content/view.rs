use chrono::NaiveDate;
use gpui::{Context, Entity, FocusHandle, ScrollHandle, Subscription, Window, prelude::*};
use gpui_component::{
    calendar::{CalendarEvent, CalendarState, Date},
    input::{InputEvent, InputState},
};

use crate::state::{TideStore, tide::update_status, update_data_and_save};

pub struct TaskView {
    pub(super) title_input: Entity<InputState>,
    pub(super) details_input: Entity<InputState>,
    pub(super) calendar_state: Entity<CalendarState>,
    pub(super) pending_due_date: Option<NaiveDate>,

    pub(super) due_picker_calendar_state: Entity<CalendarState>,
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
        let calendar_state = cx.new(|cx| CalendarState::new(window, cx));
        let due_picker_calendar_state = cx.new(|cx| CalendarState::new(window, cx));

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
                        this.pending_due_date = Some(picked);

                        update_status(cx, move |status, _| {
                            status.set_task_calendar_open(false);
                        });
                    }
                }
            },
        ));

        subs.push(cx.subscribe_in(
            &due_picker_calendar_state,
            window,
            |this: &mut Self, _, event: &CalendarEvent, window, cx| match event {
                CalendarEvent::Selected(date) => {
                    if let Some(picked) = date.start() {
                        if let Some(id) = this.due_picker_for.take() {
                            update_data_and_save(cx, "set_task_due_date", move |data, _| {
                                data.set_task_due_date(&id, Some(picked));
                            });
                            this.due_picker_calendar_state.update(cx, |state, cx| {
                                state.set_date(Date::Single(None), window, cx);
                            });
                            cx.notify();
                        }
                    }
                }
            },
        ));

        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                Self::close_form(this, window, cx);
            }
        })
        .detach();

        Self {
            title_input,
            details_input,
            calendar_state,
            pending_due_date: None,
            due_picker_calendar_state,
            due_picker_for: None,
            focus_handle: cx.focus_handle(),
            pending_scroll_handle: ScrollHandle::new(),
            batch_count: 0,
            subtask_batch_count: 0,
            completed_expanded: false,
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
    }

    pub(super) fn set_pending_due_date(
        &mut self,
        date: Option<NaiveDate>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_due_date = date;
        self.calendar_state.update(cx, |state, cx| {
            let d = match date {
                Some(d) => Date::Single(Some(d)),
                None => Date::Single(None),
            };
            state.set_date(d, window, cx);
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
