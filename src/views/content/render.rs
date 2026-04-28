use chrono::NaiveDate;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, KeyDownEvent, MouseButton, Render, Styled,
    Window, div, img, prelude::*, px, relative, rgba,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, h_flex,
    input::Escape,
    scroll::{ScrollableElement, Scrollbar},
    v_flex,
};
use rust_i18n::t;

use crate::{
    components::TaskForm,
    helpers::{i18n_content, interactive_accent, locale},
    state::{Task, TideDataStore, TideStore, update_data_and_save},
};

use super::{
    drag::{DragSubTask, DragTask},
    view::TaskView,
};

#[derive(Clone, Copy)]
enum ContentEmptyState {
    NoTasks,
    AllCompleted,
}

impl ContentEmptyState {
    fn for_task_counts(
        has_pending: bool,
        has_completed: bool,
        show_add_task_btn: bool,
    ) -> Option<Self> {
        if has_pending || !show_add_task_btn {
            return None;
        }

        if has_completed {
            Some(Self::AllCompleted)
        } else {
            Some(Self::NoTasks)
        }
    }

    fn view_id(self) -> &'static str {
        match self {
            Self::NoTasks => "empty-task-state",
            Self::AllCompleted => "completed-task-state",
        }
    }

    fn image_path(self) -> &'static str {
        match self {
            Self::NoTasks => "illustration/empty_task.svg",
            Self::AllCompleted => "illustration/completed_task.svg",
        }
    }

    fn title_key(self) -> &'static str {
        match self {
            Self::NoTasks => "empty_task_title",
            Self::AllCompleted => "completed_task_title",
        }
    }

    fn desc_key(self) -> &'static str {
        match self {
            Self::NoTasks => "empty_task_desc",
            Self::AllCompleted => "completed_task_desc",
        }
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

        let mut completed_items: Vec<(String, String, Option<NaiveDate>, bool)> = Vec::new();
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
        let empty_state = ContentEmptyState::for_task_counts(
            !pending.is_empty(),
            !completed_items.is_empty(),
            show_add_task_btn,
        );

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
            let accent = interactive_accent(cx.theme());
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
                                            .when_some(empty_state, |t, state| {
                                                t.child(Self::render_empty_state(cx, state))
                                            })
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

impl TaskView {
    fn render_empty_state(cx: &mut Context<Self>, state: ContentEmptyState) -> AnyElement {
        let fg = cx.theme().foreground;
        let muted_fg = cx.theme().muted_foreground;

        div()
            .id(state.view_id())
            .flex_1()
            .min_h(px(260.))
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(img(state.image_path()).w(px(360.)).max_w_full())
            .child(
                v_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight(500.))
                            .text_color(fg)
                            .child(i18n_content(cx, state.title_key())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted_fg)
                            .child(i18n_content(cx, state.desc_key())),
                    ),
            )
            .into_any_element()
    }
}
