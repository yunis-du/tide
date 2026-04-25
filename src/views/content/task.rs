use chrono::{Datelike, NaiveDate};
use gpui::{
    AnyElement, App, Context, ElementId, Entity, FocusHandle, FontWeight, IntoElement,
    KeyDownEvent, MouseButton, Render, ScrollHandle, Styled, Subscription, Window, anchored,
    deferred, div, prelude::*, px, relative, rgba, white,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, InteractiveElementExt, Sizable, WindowExt,
    button::{Button, ButtonVariant, ButtonVariants},
    calendar::{Calendar, CalendarEvent, CalendarState, Date},
    dialog::DialogButtonProps,
    h_flex,
    input::{Escape, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    scroll::{ScrollableElement, Scrollbar},
    v_flex,
};
use rust_i18n::t;

use crate::{
    assets::CustomIconName,
    components::{DateTag, RadioButton, TaskForm},
    helpers::{i18n_content, locale, weekday_label},
    state::{Task, TideDataStore, TideStore, tide::update_status, update_data_and_save},
};

#[derive(Clone)]
pub struct DragTask {
    pub id: String,
    pub title: String,
}

impl Render for DragTask {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bg = cx.theme().background;
        let border = cx.theme().border;
        let fg = cx.theme().foreground;

        h_flex()
            .w(px(320.))
            .px_3()
            .py_2()
            .gap_3()
            .items_center()
            .bg(bg)
            .border_1()
            .border_color(border)
            .rounded_lg()
            .shadow_md()
            .child(
                div()
                    .size_5()
                    .rounded_full()
                    .flex_shrink_0()
                    .border_2()
                    .border_color(border),
            )
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(fg)
                    .child(self.title.clone()),
            )
    }
}

#[derive(Clone)]
pub struct DragSubTask {
    pub id: String,
    pub parent_id: String,
    pub title: String,
}

impl Render for DragSubTask {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bg = cx.theme().background;
        let border = cx.theme().border;
        let fg = cx.theme().foreground;

        h_flex()
            .w(px(280.))
            .px_3()
            .py_1p5()
            .gap_3()
            .items_center()
            .bg(bg)
            .border_1()
            .border_color(border)
            .rounded_lg()
            .shadow_md()
            .child(
                div()
                    .size_4()
                    .rounded_full()
                    .flex_shrink_0()
                    .border_2()
                    .border_color(border),
            )
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(fg)
                    .child(self.title.clone()),
            )
    }
}

pub struct TaskView {
    title_input: Entity<InputState>,
    details_input: Entity<InputState>,
    calendar_state: Entity<CalendarState>,
    pending_due_date: Option<NaiveDate>,

    due_picker_calendar_state: Entity<CalendarState>,
    due_picker_for: Option<String>,
    focus_handle: FocusHandle,
    pending_scroll_handle: ScrollHandle,

    batch_count: usize,
    subtask_batch_count: usize,
    completed_expanded: bool,
    hovered_task_id: Option<String>,
    selected_task_id: Option<String>,
    dragging_task_id: Option<String>,
    hovered_subtask_id: Option<String>,
    selected_subtask_id: Option<String>,
    dragging_subtask_id: Option<String>,

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

    fn reset_pending_due_date(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pending_due_date = None;
        self.calendar_state.update(cx, |state, cx| {
            state.set_date(Date::Single(None), window, cx);
        });
    }

    fn set_pending_due_date(
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

    pub fn on_add_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        Self::close_form(self, window, cx);

        self.batch_count = 0;
        self.title_input.update(cx, |inp, cx| {
            inp.set_value("", window, cx);
            inp.focus(window, cx);
        });
        self.details_input.update(cx, |inp, cx| {
            inp.set_value("", window, cx);
        });
        self.reset_pending_due_date(window, cx);
        update_status(cx, move |status, _| {
            status.set_show_add_task_btn(false);
        });
        self.pending_scroll_handle.scroll_to_top_of_item(0);
        cx.notify();
    }

    fn details_preview(details: &str) -> String {
        let mut lines = details.lines();
        let first = lines.next();
        let second = lines.next();
        let has_more = lines.next().is_some();

        match (first, second, has_more) {
            (Some(l1), Some(l2), true) => format!("{l1}\n{l2}..."),
            _ => details.to_string(),
        }
    }

    fn enter_task(this: &mut Self, window: &mut Window, cx: &mut Context<Self>) {
        let edit_task_id = cx.global::<TideStore>().read(cx).status().edit_task_id();

        let title = this.title_input.read(cx).value().to_string();
        let trimmed = title.trim().to_string();
        if !trimmed.is_empty() {
            let list_id = cx
                .global::<TideDataStore>()
                .read(cx)
                .default_group_id_for_creation();
            let details = this.details_input.read(cx).value().to_string();
            let details_trimmed = details.trim().to_string();
            let mut task = Task::new(list_id, trimmed);
            if !details_trimmed.is_empty() {
                task.details = Some(details_trimmed);
            }
            task.due_date = this.pending_due_date;
            let idx = this.batch_count;
            this.batch_count += 1;
            let etid = edit_task_id.clone();
            update_data_and_save(cx, "create_task", move |data, _| {
                if let Some(tid) = etid {
                    task.id = tid;
                    data.update_task(task);
                } else {
                    data.insert_task(idx, task);
                }
            });
        } else {
            update_status(cx, move |status, _| {
                status.set_edit_task_id(None);
                status.set_show_add_task_btn(true);
            });
        }

        this.title_input.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        this.details_input.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        this.reset_pending_due_date(window, cx);

        if edit_task_id.is_some() {
            update_status(cx, move |status, _| {
                status.set_edit_task_id(None);
                status.set_show_add_task_btn(true);
            });
        }

        cx.notify();
    }

    fn close_form(this: &mut Self, window: &mut Window, cx: &mut Context<Self>) {
        let status = cx.global::<TideStore>().read(cx).status();
        let is_subtask_mode =
            status.adding_subtask_for().is_some() || status.edit_subtask_id().is_some();

        if is_subtask_mode {
            Self::enter_subtask(this, window, cx);
            this.subtask_batch_count = 0;
            update_status(cx, move |status, _| {
                status.set_adding_subtask_for(None);
                status.set_edit_subtask_id(None);
            });
        } else {
            Self::enter_task(this, window, cx);
        }
        this.selected_task_id = None;
        this.selected_subtask_id = None;
        this.hovered_task_id = None;
        this.hovered_subtask_id = None;
        update_status(cx, move |status, _| {
            status.set_show_add_task_btn(true);
        });
        cx.notify();
    }

    fn enter_subtask(this: &mut Self, window: &mut Window, cx: &mut Context<Self>) {
        let status = cx.global::<TideStore>().read(cx).status();
        let adding_for = status.adding_subtask_for();
        let edit_id = status.edit_subtask_id();

        let title = this.title_input.read(cx).value().to_string();
        let trimmed = title.trim().to_string();
        let details = this.details_input.read(cx).value().to_string();
        let details_trimmed = details.trim().to_string();
        let details_opt = if details_trimmed.is_empty() {
            None
        } else {
            Some(details_trimmed)
        };

        if !trimmed.is_empty() {
            if let Some(eid) = edit_id.clone() {
                let title_clone = trimmed.clone();
                let details_clone = details_opt.clone();
                let due = this.pending_due_date;
                update_data_and_save(cx, "update_subtask", move |data, _| {
                    data.set_subtask_text(&eid, title_clone.clone(), details_clone.clone());
                    data.set_task_due_date(&eid, due);
                });
            } else if let Some(parent_id) = adding_for.clone() {
                let mut sub = Task::new(String::new(), trimmed);
                sub.details = details_opt;
                sub.due_date = this.pending_due_date;
                let pid = parent_id.clone();
                update_data_and_save(cx, "create_subtask", move |data, _| {
                    let len = data.subtasks_of(&pid).len();
                    data.insert_subtask(&pid, len, sub.clone());
                });
                this.subtask_batch_count += 1;
            }
        }

        this.title_input.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        this.details_input.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        this.reset_pending_due_date(window, cx);

        if edit_id.is_some() {
            update_status(cx, move |status, _| {
                status.set_edit_subtask_id(None);
            });
        }

        cx.notify();
    }

    fn open_add_subtask(
        this: &mut Self,
        parent_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Self::close_form(this, window, cx);

        let pid = parent_id.clone();
        update_status(cx, move |status, _| {
            status.set_adding_subtask_for(Some(pid.clone()));
            status.set_show_add_task_btn(true);
            status.set_edit_task_id(None);
        });
        this.subtask_batch_count = 0;
        this.selected_task_id = Some(parent_id);

        this.title_input.update(cx, |inp, cx| {
            inp.set_value("", window, cx);
            inp.focus(window, cx);
        });
        this.details_input.update(cx, |inp, cx| {
            inp.set_value("", window, cx);
        });
        this.reset_pending_due_date(window, cx);
        cx.notify();
    }

    fn open_edit_subtask(
        this: &mut Self,
        sub: &Task,
        parent_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Self::close_form(this, window, cx);

        this.selected_task_id = Some(parent_id);
        let sid = sub.id.clone();
        let title = sub.title.clone();
        let details = sub.details.clone().unwrap_or_default();
        this.title_input.update(cx, |state, cx| {
            state.set_value(&title, window, cx);
            state.focus(window, cx);
        });
        this.details_input.update(cx, |state, cx| {
            state.set_value(&details, window, cx);
        });
        this.set_pending_due_date(sub.due_date, window, cx);
        update_status(cx, move |status, _| {
            status.set_edit_subtask_id(Some(sid.clone()));
            status.set_adding_subtask_for(None);
            status.set_show_add_task_btn(true);
            status.set_edit_task_id(None);
        });
        cx.notify();
    }

    fn edit_selected_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(selected_sub_id) = self.selected_subtask_id.clone() {
            let sub = cx
                .global::<TideDataStore>()
                .read(cx)
                .tasks
                .iter()
                .find(|t| t.id == selected_sub_id && !t.is_completed)
                .cloned();
            if let Some(subtask) = sub {
                let parent_id = subtask.parent_id.clone().unwrap_or_default();
                if !parent_id.is_empty() {
                    Self::open_edit_subtask(self, &subtask, parent_id, window, cx);
                    return;
                }
            }
        }

        let Some(selected_id) = self.selected_task_id.clone() else {
            return;
        };
        let task = cx
            .global::<TideDataStore>()
            .read(cx)
            .tasks
            .iter()
            .find(|t| t.id == selected_id && !t.is_completed)
            .cloned();
        let Some(task) = task else {
            return;
        };
        let task_title = task.title.clone();
        let task_details = task.details.clone().unwrap_or_default();
        let task_due = task.due_date;
        let task_id = task.id.clone();

        Self::close_form(self, window, cx);

        self.title_input.update(cx, |state, cx| {
            state.set_value(&task_title, window, cx);
            state.focus(window, cx);
        });
        self.details_input.update(cx, |state, cx| {
            state.set_value(&task_details, window, cx);
        });
        self.set_pending_due_date(task_due, window, cx);

        let id = task_id;
        update_status(cx, move |status, _| {
            status.set_edit_task_id(Some(id));
            status.set_show_add_task_btn(true);
        });
        cx.notify();
    }

    fn delete_selected_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(selected_sub_id) = self.selected_subtask_id.clone() {
            Self::open_delete_confirm(selected_sub_id, true, window, cx);
            return;
        }

        let Some(selected_id) = self.selected_task_id.clone() else {
            return;
        };
        Self::open_delete_confirm(selected_id, false, window, cx);
    }
}

// render functions
impl TaskView {
    fn render_subtask_form(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .pl(px(20.))
            .child(
                TaskForm::new(self.title_input.clone(), self.details_input.clone())
                    .pending_due_date(self.pending_due_date)
                    .calendar_state(self.calendar_state.clone())
                    .on_set_due_date(cx.listener(|this, date: &Option<NaiveDate>, window, cx| {
                        this.set_pending_due_date(*date, window, cx);
                    }))
                    .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                        Self::close_form(this, window, cx);
                    })),
            )
            .into_any_element()
    }

    fn render_pending_task_row(
        &self,
        cx: &mut Context<Self>,
        task: &Task,
        subtasks: &[Task],
    ) -> AnyElement {
        let task_id = task.id.clone();
        let tid_edit = task.id.clone();
        let tid_selected = task.id.clone();
        let task_title = task.title.clone();
        let task_details = task.details.clone().unwrap_or_default();
        let task_due = task.due_date;
        let tid_check = task.id.clone();
        let tid_star = task.id.clone();
        let is_completed = task.is_completed;
        let is_starred = task.is_starred;

        let list_even = cx.theme().list_even;
        let muted_fg = cx.theme().muted_foreground;
        let fg = cx.theme().foreground;

        let edit_task_id = cx.global::<TideStore>().read(cx).status().edit_task_id();

        let check_color = if is_completed { muted_fg } else { fg };

        let is_hovered = self.hovered_task_id.as_deref() == Some(&task.id);
        let is_selected = self.selected_task_id.as_deref() == Some(&task.id);
        let is_editing = edit_task_id.as_deref() == Some(&task.id);

        let root = v_flex().w_full();

        let task_row = if is_editing {
            root.child(
                TaskForm::new(self.title_input.clone(), self.details_input.clone())
                    .pending_due_date(self.pending_due_date)
                    .calendar_state(self.calendar_state.clone())
                    .on_set_due_date(cx.listener(|this, date: &Option<NaiveDate>, window, cx| {
                        this.set_pending_due_date(*date, window, cx);
                    }))
                    .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                        Self::close_form(this, window, cx);
                    })),
            )
        } else {
            let drag_payload = DragTask {
                id: task.id.clone(),
                title: task.title.clone(),
            };
            let drop_target_id = task.id.clone();
            let drop_target_id_sub = task.id.clone();
            let drag_start_id = task.id.clone();
            let weak = cx.entity().downgrade();
            let accent = cx.theme().info_active;

            root.child(
                h_flex()
                    .id(ElementId::Name(format!("task-{}", task.id).into()))
                    .w_full()
                    .px_3()
                    .py_2()
                    .gap_3()
                    .items_center()
                    .rounded_lg()
                    .when(is_hovered || is_selected, |s| s.bg(list_even))
                    .hover(|s| s.bg(list_even))
                    .on_drag(drag_payload, move |drag, _, _, cx| {
                        cx.stop_propagation();
                        let id = drag_start_id.clone();
                        weak.update(cx, |this, cx| {
                            this.dragging_task_id = Some(id);
                            cx.notify();
                        })
                        .ok();
                        cx.new(|_| drag.clone())
                    })
                    .drag_over::<DragTask>(move |this, _, _, _| {
                        this.rounded_none().border_t_2().border_color(accent)
                    })
                    .drag_over::<DragSubTask>(move |this, _, _, _| {
                        this.rounded_none().border_t_2().border_color(accent)
                    })
                    .on_drop(cx.listener(move |this, drag: &DragTask, _, cx| {
                        let from_id = drag.id.clone();
                        let before_id = drop_target_id.clone();
                        this.dragging_task_id = None;
                        update_data_and_save(cx, "reorder_task", move |data, _| {
                            data.reorder_task_before(&from_id, &before_id);
                        });
                        cx.notify();
                    }))
                    .on_drop(cx.listener(move |this, drag: &DragSubTask, _, cx| {
                        let from_id = drag.id.clone();
                        let before_id = drop_target_id_sub.clone();
                        this.dragging_subtask_id = None;
                        update_data_and_save(cx, "promote_subtask", move |data, _| {
                            data.promote_subtask_to_task(&from_id, &before_id);
                        });
                        cx.notify();
                    }))
                    .on_hover(cx.listener(move |this, is_hov: &bool, _, cx| {
                        let current = this.hovered_task_id.as_deref();
                        let next = if *is_hov {
                            Some(task_id.clone())
                        } else if current == Some(task_id.as_str()) {
                            None
                        } else {
                            this.hovered_task_id.clone()
                        };

                        if this.hovered_task_id != next {
                            this.hovered_task_id = next;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.selected_task_id = Some(tid_selected.clone());
                        cx.notify();
                    }))
                    .on_mouse_down_out(cx.listener(move |this, _, _window, cx| {
                        this.selected_task_id = None;
                        cx.notify();
                    }))
                    .child(
                        RadioButton::new(task.id.clone())
                            .large()
                            .on_click(move |_, _, cx| {
                                let id = tid_check.clone();
                                update_data_and_save(cx, "toggle_done", move |data, _| {
                                    data.toggle_task_completion(&id);
                                });
                            }),
                    )
                    .child(
                        v_flex()
                            .id(ElementId::Name(format!("task-label-{}", task.id).into()))
                            .flex_1()
                            .gap_0p5()
                            .items_start()
                            .on_double_click(cx.listener(move |this, _, window, cx| {
                                Self::close_form(this, window, cx);

                                this.selected_task_id = Some(tid_edit.clone());

                                let title = task_title.clone();
                                let details = task_details.clone();
                                this.title_input.update(cx, |state, cx| {
                                    state.set_value(&title, window, cx);
                                    state.focus(window, cx);
                                });
                                this.details_input.update(cx, |state, cx| {
                                    state.set_value(&details, window, cx);
                                });
                                this.set_pending_due_date(task_due, window, cx);

                                let id = tid_edit.clone();
                                update_status(cx, move |status, _| {
                                    status.set_edit_task_id(Some(id));
                                    status.set_show_add_task_btn(true);
                                });
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .w_full()
                                    .text_sm()
                                    .text_color(check_color)
                                    .when(is_completed, |t| t.line_through())
                                    .child(task.title.clone()),
                            )
                            .when_some(task.details.clone(), |t, details| {
                                let preview = Self::details_preview(&details);
                                t.child(
                                    div()
                                        .w_full()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(muted_fg)
                                        .when(is_completed, |t| t.line_through())
                                        .line_clamp(2)
                                        .child(preview),
                                )
                            })
                            .when_some(task.due_date, |t, date| {
                                let tid_picker = task.id.clone();
                                t.child(
                                    h_flex()
                                        .id(ElementId::Name(
                                            format!("task-date-{}", task.id).into(),
                                        ))
                                        .flex_none()
                                        .relative()
                                        .mt_0p5()
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.due_picker_for = Some(tid_picker.clone());
                                            this.due_picker_calendar_state.update(
                                                cx,
                                                |state, cx| {
                                                    state.set_date(
                                                        Date::Single(Some(date)),
                                                        window,
                                                        cx,
                                                    );
                                                },
                                            );
                                            cx.notify();
                                        }))
                                        .child(DateTag::new(date))
                                        .when_some(
                                            self.render_due_picker(cx, &task.id),
                                            |t, picker| t.child(picker),
                                        ),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .min_h(px(24.))
                            .items_center()
                            .child(
                                div()
                                    .opacity(if is_hovered || is_selected { 1.0 } else { 0.0 })
                                    .when(!(is_hovered || is_selected), |d| d.cursor_default())
                                    .child(Self::render_options_menu(cx, task)),
                            )
                            .when(is_starred || is_hovered || is_selected, |t| {
                                let icon = if is_starred {
                                    CustomIconName::Star
                                } else {
                                    CustomIconName::StarOutline
                                };
                                t.child(
                                    Button::new(ElementId::Name(
                                        format!("star-{}", tid_star).into(),
                                    ))
                                    .icon(icon)
                                    .ghost()
                                    .small()
                                    .cursor_pointer()
                                    .on_click(
                                        move |_, _, cx| {
                                            let id = tid_star.clone();
                                            update_data_and_save(
                                                cx,
                                                "toggle_star",
                                                move |data, _| {
                                                    data.toggle_task_star(&id);
                                                },
                                            );
                                        },
                                    ),
                                )
                            })
                            .when(task.due_date.is_none(), |t| {
                                t.relative()
                                    .when_some(self.render_due_picker(cx, &task.id), |t, picker| {
                                        t.child(picker)
                                    })
                            }),
                    ),
            )
        };

        let (adding_subtask_for, edit_subtask_id) = {
            let status = cx.global::<TideStore>().read(cx).status();
            (status.adding_subtask_for(), status.edit_subtask_id())
        };
        let is_adding_sub_here = adding_subtask_for.as_deref() == Some(&task.id);
        let dragging_sub_id = self.dragging_subtask_id.clone();

        let mut sub_els: Vec<AnyElement> = Vec::new();
        let mut has_pending_sub = false;
        for sub in subtasks {
            if sub.is_completed {
                continue;
            }
            has_pending_sub = true;
            if dragging_sub_id.as_deref() == Some(sub.id.as_str()) {
                continue;
            }
            if edit_subtask_id.as_deref() == Some(sub.id.as_str()) {
                sub_els.push(self.render_subtask_form(cx));
            } else {
                sub_els.push(self.render_subtask_row(cx, &task.id, sub));
            }
        }

        let mut row = task_row.children(sub_els);
        if has_pending_sub {
            let parent_id_for_end = task.id.clone();
            let end_accent = cx.theme().info_active;
            let sub_end_drop = div()
                .id(ElementId::Name(format!("sub-drop-end-{}", task.id).into()))
                .w_full()
                .pl_8()
                .min_h(px(12.))
                .drag_over::<DragSubTask>(move |this, drag: &DragSubTask, _, _| {
                    if drag.parent_id == parent_id_for_end {
                        this.border_t_2().border_color(end_accent)
                    } else {
                        this
                    }
                })
                .on_drop(cx.listener({
                    let pid = task.id.clone();
                    move |this, drag: &DragSubTask, _, cx| {
                        this.dragging_subtask_id = None;
                        if drag.parent_id != pid {
                            cx.notify();
                            return;
                        }
                        let from_id = drag.id.clone();
                        update_data_and_save(cx, "reorder_subtask", move |data, _| {
                            data.reorder_subtask_before(&from_id, "");
                        });
                        cx.notify();
                    }
                }));
            row = row.child(sub_end_drop);
        }
        if is_adding_sub_here {
            row = row.child(self.render_subtask_form(cx));
        }

        row.into_any_element()
    }

    fn render_completed_row(
        &self,
        cx: &mut Context<Self>,
        id: String,
        title: String,
        completed_at: Option<chrono::NaiveDate>,
        is_subtask: bool,
    ) -> AnyElement {
        let accent = cx.theme().info_active;
        let list_even = cx.theme().list_even;
        let muted_fg = cx.theme().muted_foreground;
        let locale = locale(cx);

        let task_id = id.clone();
        let is_hovered = self.hovered_task_id.as_deref() == Some(&id);
        let id_for_undo = id.clone();
        let id_for_delete = id.clone();
        h_flex()
            .id(ElementId::Name(format!("done-task-{}", id).into()))
            .w_full()
            .px_3()
            .py_2()
            .gap_3()
            .items_center()
            .rounded_lg()
            .when(is_hovered, |s| s.bg(list_even))
            .hover(|s| s.bg(list_even))
            .on_hover(cx.listener(move |this, is_hov: &bool, _, cx| {
                let current = this.hovered_task_id.as_deref();
                let next = if *is_hov {
                    Some(task_id.clone())
                } else if current == Some(task_id.as_str()) {
                    None
                } else {
                    this.hovered_task_id.clone()
                };

                if this.hovered_task_id != next {
                    this.hovered_task_id = next;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .id(ElementId::Name(format!("done-chk-{}", id).into()))
                    .size_5()
                    .rounded_full()
                    .flex_shrink_0()
                    .bg(accent)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(IconName::Check).size_3().text_color(white())),
            )
            .child(
                v_flex()
                    .flex_1()
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted_fg)
                            .line_through()
                            .child(title),
                    )
                    .when_some(completed_at, |t, date: chrono::NaiveDate| {
                        let prefix: String = i18n_content(cx, "completed_date_prefix");
                        let date_str: String = t!(
                            "content.completed_date",
                            month = date.month(),
                            day = date.day(),
                            weekday = weekday_label(date.weekday(), locale.as_str()),
                            locale = locale
                        )
                        .into();
                        t.child(
                            div()
                                .text_xs()
                                .text_color(muted_fg)
                                .child(format!("{prefix}{date_str}")),
                        )
                    }),
            )
            .when(is_hovered, |t| {
                t.child(
                    h_flex()
                        .gap_1()
                        .min_h(px(24.))
                        .items_center()
                        .child(
                            Button::new(ElementId::Name(format!("done-undo-{}", id).into()))
                                .icon(IconName::Undo)
                                .ghost()
                                .small()
                                .cursor_pointer()
                                .tooltip(i18n_content(cx, "undo"))
                                .on_click(move |_, _, cx| {
                                    let id = id_for_undo.clone();
                                    update_data_and_save(cx, "undo_done", move |data, _| {
                                        data.toggle_task_completion(&id);
                                    });
                                }),
                        )
                        .child(
                            Button::new(ElementId::Name(format!("done-delete-{}", id).into()))
                                .icon(CustomIconName::Trash)
                                .ghost()
                                .small()
                                .cursor_pointer()
                                .tooltip(i18n_content(cx, "delete"))
                                .on_click(move |_, window, cx| {
                                    let id = id_for_delete.clone();
                                    Self::open_delete_confirm(id, is_subtask, window, cx);
                                }),
                        ),
                )
            })
            .into_any_element()
    }

    fn open_delete_confirm(id: String, is_subtask: bool, window: &mut Window, cx: &mut App) {
        let title_key = if is_subtask {
            "delete_subtask_title"
        } else {
            "delete_task_title"
        };
        let desc_key = if is_subtask {
            "delete_subtask_desc"
        } else {
            "delete_task_desc"
        };
        let action_label = if is_subtask {
            "delete_subtask"
        } else {
            "delete_task"
        };

        window.open_dialog(cx, move |dialog, window, cx| {
            let id_for_del = id.clone();
            let dialog_width = px(360.);
            let dialog_height = px(160.);
            let margin_top = ((window.viewport_size().height - dialog_height) / 2.).max(px(0.));
            dialog
                .title(i18n_content(cx, title_key))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(i18n_content(cx, desc_key)),
                )
                .w(dialog_width)
                .margin_top(margin_top)
                .confirm()
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(i18n_content(cx, "confirm_delete"))
                        .cancel_text(i18n_content(cx, "cancel"))
                        .ok_variant(ButtonVariant::Danger),
                )
                .on_ok(move |_, _, cx| {
                    let id = id_for_del.clone();
                    if is_subtask {
                        update_data_and_save(cx, action_label, move |data, _| {
                            data.remove_subtask(&id);
                        });
                    } else {
                        update_data_and_save(cx, action_label, move |data, _| {
                            data.remove_task(&id);
                        });
                    }
                    true
                })
        });
    }

    fn render_due_picker(&self, cx: &mut Context<Self>, id: &str) -> Option<AnyElement> {
        if self.due_picker_for.as_deref() != Some(id) {
            return None;
        }
        let cal_state = self.due_picker_calendar_state.clone();
        let id_close = id.to_string();
        let border = cx.theme().border;
        let popover_bg = cx.theme().popover;
        let popover_fg = cx.theme().popover_foreground;
        let radius = cx.theme().radius_lg;
        Some(
            deferred(
                anchored().snap_to_window_with_margin(px(8.)).child(
                    div()
                        .id(ElementId::Name(format!("due-picker-pop-{}", id).into()))
                        .occlude()
                        .mt_1()
                        .p_2()
                        .border_1()
                        .border_color(border)
                        .shadow_lg()
                        .rounded(radius)
                        .bg(popover_bg)
                        .text_color(popover_fg)
                        .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                            if this.due_picker_for.as_deref() == Some(&id_close) {
                                this.due_picker_for = None;
                                cx.notify();
                            }
                        }))
                        .child(Calendar::new(&cal_state).number_of_months(1)),
                ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    fn render_options_menu(cx: &mut Context<Self>, task: &Task) -> AnyElement {
        let tid_due = task.id.clone();
        let tid_del = task.id.clone();
        let tid_sub = task.id.clone();
        let tid_selected = task.id.clone();
        let task_due = task.due_date;
        let add_due_label = i18n_content(cx, "add_due_date");
        let add_subtask_label = i18n_content(cx, "add_subtask");
        let delete_label = i18n_content(cx, "delete");
        let weak = cx.entity().downgrade();

        Button::new(ElementId::Name(format!("task-menu-{}", task.id).into()))
            .icon(IconName::Ellipsis)
            .ghost()
            .small()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.selected_task_id.as_deref() != Some(tid_selected.as_str()) {
                    this.selected_task_id = Some(tid_selected.clone());
                    cx.notify();
                }
            }))
            .dropdown_menu(move |menu: PopupMenu, _, _| {
                let del = tid_del.clone();
                let due = tid_due.clone();
                let sub = tid_sub.clone();
                let weak_due = weak.clone();
                let weak = weak.clone();
                menu.item(
                    PopupMenuItem::new(add_due_label.clone())
                        .icon(Icon::new(IconName::Calendar))
                        .on_click(move |_, window, cx| {
                            let id = due.clone();
                            weak_due
                                .update(cx, move |this, cx| {
                                    this.due_picker_for = Some(id);
                                    this.due_picker_calendar_state.update(cx, |state, cx| {
                                        let d = match task_due {
                                            Some(d) => Date::Single(Some(d)),
                                            None => Date::Single(None),
                                        };
                                        state.set_date(d, window, cx);
                                    });
                                    cx.notify();
                                })
                                .ok();
                        }),
                )
                .item(
                    PopupMenuItem::new(add_subtask_label.clone())
                        .icon(Icon::new(IconName::Plus))
                        .on_click(move |_, window, cx| {
                            let pid = sub.clone();
                            weak.update(cx, |this, cx| {
                                Self::open_add_subtask(this, pid, window, cx);
                            })
                            .ok();
                        }),
                )
                .separator()
                .item(
                    PopupMenuItem::new(delete_label.clone())
                        .icon(Icon::new(IconName::Delete))
                        .on_click(move |_, window, cx| {
                            let id = del.clone();
                            Self::open_delete_confirm(id, false, window, cx);
                        }),
                )
            })
            .into_any_element()
    }

    fn render_subtask_options_menu(
        cx: &mut Context<Self>,
        parent_id: &str,
        sub: &Task,
    ) -> AnyElement {
        let sid_due = sub.id.clone();
        let sid_del = sub.id.clone();
        let sub_due = sub.due_date;
        let add_due_label = i18n_content(cx, "add_due_date");
        let delete_label = i18n_content(cx, "delete");
        let menu_id = format!("subtask-menu-{}-{}", parent_id, sub.id);
        let sid_selected = sub.id.clone();
        let weak = cx.entity().downgrade();

        Button::new(ElementId::Name(menu_id.into()))
            .icon(IconName::Ellipsis)
            .ghost()
            .small()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.selected_subtask_id.as_deref() != Some(sid_selected.as_str()) {
                    this.selected_subtask_id = Some(sid_selected.clone());
                    cx.notify();
                }
            }))
            .dropdown_menu(move |menu: PopupMenu, _, _| {
                let due = sid_due.clone();
                let del = sid_del.clone();
                let weak_due = weak.clone();
                menu.item(
                    PopupMenuItem::new(add_due_label.clone())
                        .icon(Icon::new(IconName::Calendar))
                        .on_click(move |_, window, cx| {
                            let id = due.clone();
                            weak_due
                                .update(cx, move |this, cx| {
                                    this.due_picker_for = Some(id);
                                    this.due_picker_calendar_state.update(cx, |state, cx| {
                                        let d = match sub_due {
                                            Some(d) => Date::Single(Some(d)),
                                            None => Date::Single(None),
                                        };
                                        state.set_date(d, window, cx);
                                    });
                                    cx.notify();
                                })
                                .ok();
                        }),
                )
                .separator()
                .item(
                    PopupMenuItem::new(delete_label.clone())
                        .icon(Icon::new(IconName::Delete))
                        .on_click(move |_, window, cx| {
                            let id = del.clone();
                            Self::open_delete_confirm(id, true, window, cx);
                        }),
                )
            })
            .into_any_element()
    }

    fn render_subtask_row(
        &self,
        cx: &mut Context<Self>,
        parent_id: &str,
        sub: &Task,
    ) -> AnyElement {
        let sid_check = sub.id.clone();
        let sid_selected = sub.id.clone();
        let sid_hover = sub.id.clone();
        let sid_star = sub.id.clone();
        let parent_for_drop = parent_id.to_string();
        let parent_for_click = parent_id.to_string();
        let sub_for_edit = sub.clone();
        let sub_title = sub.title.clone();
        let is_completed = sub.is_completed;
        let is_starred = sub.is_starred;

        let list_even = cx.theme().list_even;
        let muted_fg = cx.theme().muted_foreground;
        let fg = cx.theme().foreground;
        let accent = cx.theme().info_active;

        let check_color = if is_completed { muted_fg } else { fg };
        let is_hovered = self.hovered_subtask_id.as_deref() == Some(&sub.id);
        let is_selected = self.selected_subtask_id.as_deref() == Some(&sub.id);

        let drag_payload = DragSubTask {
            id: sub.id.clone(),
            parent_id: parent_id.to_string(),
            title: sub.title.clone(),
        };
        let drop_target_id = sub.id.clone();
        let drop_parent_id = parent_id.to_string();
        let drag_start_id = sub.id.clone();
        let weak = cx.entity().downgrade();

        h_flex()
            .id(ElementId::Name(
                format!("subtask-{}-{}", parent_id, sub.id).into(),
            ))
            .w_full()
            .pl_8()
            .pr_3()
            .py_1p5()
            .gap_3()
            .items_center()
            .rounded_lg()
            .when(is_hovered || is_selected, |s| s.bg(list_even))
            .hover(|s| s.bg(list_even))
            .on_drag(drag_payload, move |drag, _, _, cx| {
                cx.stop_propagation();
                let id = drag_start_id.clone();
                weak.update(cx, |this, cx| {
                    this.dragging_subtask_id = Some(id);
                    cx.notify();
                })
                .ok();
                cx.new(|_| drag.clone())
            })
            .drag_over::<DragSubTask>(move |this, drag: &DragSubTask, _, _| {
                if drag.parent_id == drop_parent_id {
                    this.rounded_none().border_t_2().border_color(accent)
                } else {
                    this
                }
            })
            .on_drop(cx.listener(move |this, drag: &DragSubTask, _, cx| {
                this.dragging_subtask_id = None;
                if drag.parent_id != parent_for_drop {
                    cx.notify();
                    return;
                }
                let from_id = drag.id.clone();
                let before_id = drop_target_id.clone();
                update_data_and_save(cx, "reorder_subtask", move |data, _| {
                    data.reorder_subtask_before(&from_id, &before_id);
                });
                cx.notify();
            }))
            .on_hover(cx.listener(move |this, is_hov: &bool, _, cx| {
                let next = if *is_hov {
                    Some(sid_hover.clone())
                } else if this.hovered_subtask_id.as_deref() == Some(sid_hover.as_str()) {
                    None
                } else {
                    this.hovered_subtask_id.clone()
                };
                if this.hovered_subtask_id != next {
                    this.hovered_subtask_id = next;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _window, cx| {
                this.selected_subtask_id = Some(sid_selected.clone());
                cx.notify();
            }))
            .on_mouse_down_out(cx.listener(move |this, _, _window, cx| {
                this.selected_subtask_id = None;
                cx.notify();
            }))
            .child(
                RadioButton::new(sub.id.clone())
                    .large()
                    .on_click(move |_, _, cx| {
                        let id = sid_check.clone();
                        update_data_and_save(cx, "toggle_subtask_done", move |data, _| {
                            data.toggle_task_completion(&id);
                        });
                    }),
            )
            .child(
                v_flex()
                    .id(ElementId::Name(format!("sub-label-{}", sub.id).into()))
                    .flex_1()
                    .gap_0p5()
                    .items_start()
                    .on_double_click(cx.listener(move |this, _, window, cx| {
                        let parent = parent_for_click.clone();
                        Self::open_edit_subtask(this, &sub_for_edit, parent, window, cx);
                    }))
                    .child(
                        div()
                            .w_full()
                            .text_sm()
                            .text_color(check_color)
                            .when(is_completed, |t| t.line_through())
                            .child(sub_title),
                    )
                    .when_some(sub.details.clone(), |t, details| {
                        let preview = Self::details_preview(&details);
                        t.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .text_xs()
                                .text_color(muted_fg)
                                .when(is_completed, |t| t.line_through())
                                .line_clamp(2)
                                .child(preview),
                        )
                    })
                    .when_some(sub.due_date, |t, date| {
                        let sid_picker = sub.id.clone();
                        t.child(
                            h_flex()
                                .id(ElementId::Name(format!("sub-date-{}", sub.id).into()))
                                .flex_none()
                                .relative()
                                .mt_0p5()
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.due_picker_for = Some(sid_picker.clone());
                                    this.due_picker_calendar_state.update(cx, |state, cx| {
                                        state.set_date(Date::Single(Some(date)), window, cx);
                                    });
                                    cx.notify();
                                }))
                                .child(DateTag::new(date))
                                .when_some(self.render_due_picker(cx, &sub.id), |t, picker| {
                                    t.child(picker)
                                }),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .min_h(px(24.))
                    .items_center()
                    .child(
                        div()
                            .opacity(if is_hovered || is_selected { 1.0 } else { 0.0 })
                            .when(!(is_hovered || is_selected), |d| d.cursor_default())
                            .child(Self::render_subtask_options_menu(cx, parent_id, sub)),
                    )
                    .when(is_starred || is_hovered || is_selected, |t| {
                        let icon = if is_starred {
                            CustomIconName::Star
                        } else {
                            CustomIconName::StarOutline
                        };
                        t.child(
                            Button::new(ElementId::Name(format!("sub-star-{}", sid_star).into()))
                                .icon(icon)
                                .ghost()
                                .small()
                                .cursor_pointer()
                                .on_click(move |_, _, cx| {
                                    let id = sid_star.clone();
                                    update_data_and_save(
                                        cx,
                                        "toggle_subtask_star",
                                        move |data, _| {
                                            data.toggle_task_star(&id);
                                        },
                                    );
                                }),
                        )
                    })
                    .when(sub.due_date.is_none(), |t| {
                        t.relative()
                            .when_some(self.render_due_picker(cx, &sub.id), |t, picker| {
                                t.child(picker)
                            })
                    }),
            )
            .into_any_element()
    }
}

impl Render for TaskView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fg = cx.theme().foreground;
        let muted_fg = cx.theme().muted_foreground;

        if !cx.has_active_drag() {
            if self.dragging_task_id.is_some() {
                self.dragging_task_id = None;
            }
            if self.dragging_subtask_id.is_some() {
                self.dragging_subtask_id = None;
            }
        }

        let show_add_task_btn = cx
            .global::<TideStore>()
            .read(cx)
            .status()
            .show_add_task_btn();

        let data = cx.global::<TideDataStore>().read(cx);
        let visible_tasks = data
            .visible_tasks()
            .into_iter()
            .cloned()
            .collect::<Vec<Task>>();
        let all_tasks = data.tasks.clone();

        let subtasks_of = |parent_id: &str| -> Vec<Task> {
            all_tasks
                .iter()
                .filter(|t| t.parent_id.as_deref() == Some(parent_id))
                .cloned()
                .collect()
        };

        let dragging_id = self.dragging_task_id.clone();
        let pending = visible_tasks
            .iter()
            .cloned()
            .filter(|t| !t.is_completed)
            .filter(|t| dragging_id.as_deref() != Some(t.id.as_str()))
            .collect::<Vec<Task>>();

        let split = if show_add_task_btn {
            0
        } else {
            self.batch_count.min(pending.len())
        };
        let (batch_pending, rest_pending) = pending.split_at(split);

        let mut batch_els: Vec<AnyElement> = Vec::new();
        for task in batch_pending {
            let subs = subtasks_of(&task.id);
            batch_els.push(self.render_pending_task_row(cx, task, &subs));
        }

        let mut rest_els: Vec<AnyElement> = Vec::new();
        for task in rest_pending {
            let subs = subtasks_of(&task.id);
            rest_els.push(self.render_pending_task_row(cx, task, &subs));
        }

        let mut completed_items: Vec<(String, String, Option<chrono::NaiveDate>, bool)> =
            Vec::new();
        for task in &visible_tasks {
            if task.is_completed {
                completed_items.push((
                    task.id.clone(),
                    task.title.clone(),
                    task.completed_at,
                    false,
                ));
            }
            for sub in subtasks_of(&task.id) {
                if sub.is_completed {
                    completed_items.push((
                        sub.id.clone(),
                        sub.title.clone(),
                        sub.completed_at,
                        true,
                    ));
                }
            }
        }

        let mut completed_els: Vec<AnyElement> = Vec::new();
        let completed_expanded = self.completed_expanded;
        if completed_expanded {
            for (id, title, date, is_subtask) in &completed_items {
                completed_els.push(self.render_completed_row(
                    cx,
                    id.clone(),
                    title.clone(),
                    *date,
                    *is_subtask,
                ));
            }
        }
        let completed_label: String = t!(
            "content.completed_section",
            count = completed_items.len(),
            locale = locale(cx).as_str()
        )
        .into();

        let task_form = TaskForm::new(self.title_input.clone(), self.details_input.clone())
            .pending_due_date(self.pending_due_date)
            .calendar_state(self.calendar_state.clone())
            .on_set_due_date(cx.listener(|this, date: &Option<NaiveDate>, window, cx| {
                this.set_pending_due_date(*date, window, cx);
            }))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                Self::close_form(this, window, cx);
            }));

        let add_task_btn = {
            let accent = cx.theme().info_active;
            let add_task_label = i18n_content(cx, "add_task");

            h_flex()
                .id("add-task-btn")
                .px_2()
                .py_2()
                .gap_2()
                .items_center()
                .cursor_pointer()
                .rounded_lg()
                .hover(|s| s.bg(rgba(0x00000010)))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.on_add_task(window, cx);
                }))
                .child(Icon::new(IconName::CircleCheck).size_5().text_color(accent))
                .child(div().text_sm().text_color(accent).child(add_task_label))
                .into_any_element()
        };

        let end_drop_accent = cx.theme().info_active;
        let end_drop_zone = div()
            .id("task-drop-end")
            .w_full()
            .flex_1()
            .min_h(px(40.))
            .drag_over::<DragTask>(move |this, _, _, _| {
                this.border_t_2().border_color(end_drop_accent)
            })
            .drag_over::<DragSubTask>(move |this, _, _, _| {
                this.border_t_2().border_color(end_drop_accent)
            })
            .on_drop(cx.listener(|this, drag: &DragTask, _, cx| {
                let from_id = drag.id.clone();
                this.dragging_task_id = None;
                update_data_and_save(cx, "reorder_task", move |data, _| {
                    data.reorder_task_before(&from_id, "");
                });
                cx.notify();
            }))
            .on_drop(cx.listener(|this, drag: &DragSubTask, _, cx| {
                let from_id = drag.id.clone();
                this.dragging_subtask_id = None;
                update_data_and_save(cx, "promote_subtask", move |data, _| {
                    data.promote_subtask_to_task(&from_id, "");
                });
                cx.notify();
            }));

        v_flex()
            .h_full()
            .min_h_0()
            .gap_2()
            .track_focus(&self.focus_handle)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, _| {
                    this.focus_handle.focus(window);
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let status = cx.global::<TideStore>().read(cx).status();
                let is_editing =
                    status.edit_task_id().is_some() || status.edit_subtask_id().is_some();
                let is_adding_subtask = status.adding_subtask_for().is_some();
                if is_editing || is_adding_subtask {
                    return;
                }

                match event.keystroke.key.as_str() {
                    "enter" => {
                        this.edit_selected_item(window, cx);
                        cx.stop_propagation();
                    }
                    "backspace" | "delete" => {
                        this.delete_selected_item(window, cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .on_action(cx.listener(move |this, _: &Escape, window, cx| {
                Self::close_form(this, window, cx);
            }))
            .child(
                v_flex()
                    .id("pending-panel")
                    .min_h_0()
                    .flex_1()
                    .when(completed_expanded, |t| {
                        t.flex_basis(relative(0.6)).flex_shrink_0()
                    })
                    .overflow_hidden()
                    .when(show_add_task_btn, |t| t.child(add_task_btn))
                    .child(
                        v_flex()
                            .id("pending-list")
                            .flex_1()
                            .min_h_0()
                            .relative()
                            .child(
                                v_flex()
                                    .id("pending-scroll-area")
                                    .flex()
                                    .size_full()
                                    .flex_col()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.pending_scroll_handle)
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .children(batch_els)
                                            .when(!show_add_task_btn, |t| t.child(task_form))
                                            .children(rest_els)
                                            .child(end_drop_zone),
                                    ),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .right_0()
                                    .bottom_0()
                                    .child(Scrollbar::vertical(&self.pending_scroll_handle)),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .id("completed-panel")
                    .mt_2()
                    .when(completed_expanded, |t| {
                        t.min_h_0()
                            .flex_basis(relative(0.4))
                            .flex_shrink_0()
                            .overflow_hidden()
                    })
                    .child(
                        h_flex()
                            .id("completed-hdr")
                            .px_2()
                            .py_1()
                            .gap_2()
                            .items_center()
                            .cursor_pointer()
                            .rounded_lg()
                            .hover(|s| s.bg(rgba(0x00000010)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.completed_expanded = !this.completed_expanded;
                                cx.notify();
                            }))
                            .child(
                                Icon::new(if completed_expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size_4()
                                .text_color(muted_fg),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight(500.))
                                    .text_color(fg)
                                    .child(completed_label),
                            ),
                    )
                    .when(completed_expanded, |t| {
                        t.child(
                            v_flex()
                                .id("completed-list")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scrollbar()
                                .children(completed_els),
                        )
                    }),
            )
            .into_any_element()
    }
}
