use chrono::NaiveDate;
use gpui::{AnyElement, Context, ElementId, IntoElement, Styled, div, prelude::*, px};
use gpui_component::{
    ActiveTheme, InteractiveElementExt, Sizable,
    button::{Button, ButtonVariants},
    calendar::Date,
    h_flex,
    menu::ContextMenuExt,
    v_flex,
};

use crate::{
    assets::CustomIconName,
    components::{DateTag, RadioButton, TaskForm},
    state::{Task, TideStore, tide::update_status, update_data_and_save},
};

use super::{
    drag::{DragSubTask, DragTask},
    view::TaskView,
};

impl TaskView {
    pub(super) fn render_subtask_form(&self, cx: &mut Context<Self>) -> AnyElement {
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

    pub(super) fn render_pending_task_row(
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
            let weak_menu = weak.clone();
            let accent = cx.theme().info_active;
            let menu_task_id = task.id.clone();
            let menu_task_due = task.due_date;

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
                    )
                    .context_menu(Self::task_menu_builder(
                        weak_menu,
                        menu_task_id,
                        menu_task_due,
                    )),
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

    pub(super) fn render_subtask_row(
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
        let weak_menu = weak.clone();
        let menu_sub_id = sub.id.clone();
        let menu_sub_due = sub.due_date;

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
            .context_menu(Self::subtask_menu_builder(
                weak_menu,
                menu_sub_id,
                menu_sub_due,
            ))
            .into_any_element()
    }
}
