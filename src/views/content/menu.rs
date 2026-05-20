use gpui::{
    AnyElement, Context, ElementId, IntoElement, Styled, WeakEntity, Window, anchored, deferred,
    div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    calendar::{Calendar, Date},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};

use crate::{
    components::DueTimeInput,
    helpers::{due_time_label, i18n_content, parse_due_time},
    state::{DueDate, Task, update_data_and_save},
};

use super::view::TaskView;

impl TaskView {
    pub(super) fn render_due_picker(&self, cx: &mut Context<Self>, id: &str) -> Option<AnyElement> {
        if self.due_picker_for.as_deref() != Some(id) {
            return None;
        }
        let cal_state = self.due_picker_calendar_state.clone();
        let time_input = self.due_picker_time_input.clone();
        let id_for_time = id.to_string();
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
                        .child(Calendar::new(&cal_state).number_of_months(1))
                        .child(div().pt_2().child(
                            DueTimeInput::new("task-due-time", time_input.clone()).on_select(
                                move |value, _window, cx| {
                                    let id = id_for_time.clone();
                                    let time = parse_due_time(value);
                                    update_data_and_save(
                                        cx,
                                        "set_task_due_time",
                                        move |data, _| {
                                            if let Some(task) =
                                                data.tasks.iter_mut().find(|task| task.id == id)
                                                && let Some(mut due) = task.due_date
                                            {
                                                due.time = time;
                                                task.due_date = Some(due);
                                            }
                                        },
                                    );
                                },
                            ),
                        )),
                ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    pub(super) fn task_menu_builder(
        weak: WeakEntity<Self>,
        task_id: String,
        task_due: Option<DueDate>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        move |menu, _window, cx| {
            let add_due_label = i18n_content(cx, "add_due_date");
            let add_subtask_label = i18n_content(cx, "add_subtask");
            let delete_label = i18n_content(cx, "delete");

            menu.item(
                PopupMenuItem::new(add_due_label)
                    .icon(Icon::new(IconName::Calendar))
                    .on_click({
                        let weak = weak.clone();
                        let task_id = task_id.clone();
                        move |_, window, cx| {
                            let id = task_id.clone();
                            weak.update(cx, move |this, cx| {
                                this.due_picker_for = Some(id);
                                this.due_picker_calendar_state.update(cx, |state, cx| {
                                    let d = match task_due {
                                        Some(d) => Date::Single(Some(d.date)),
                                        None => Date::Single(None),
                                    };
                                    state.set_date(d, window, cx);
                                });
                                this.due_picker_time_input.update(cx, |state, cx| {
                                    state.set_placeholder(
                                        i18n_content(cx, "due_time_placeholder"),
                                        window,
                                        cx,
                                    );
                                    let value = task_due
                                        .and_then(|due| due.time)
                                        .map(due_time_label)
                                        .unwrap_or_default();
                                    state.set_value(value, window, cx);
                                });
                                cx.notify();
                            })
                            .ok();
                        }
                    }),
            )
            .item(
                PopupMenuItem::new(add_subtask_label)
                    .icon(Icon::new(IconName::Plus))
                    .on_click({
                        let weak = weak.clone();
                        let task_id = task_id.clone();
                        move |_, window, cx| {
                            let pid = task_id.clone();
                            weak.update(cx, |this, cx| {
                                Self::open_add_subtask(this, pid, window, cx);
                            })
                            .ok();
                        }
                    }),
            )
            .separator()
            .item(
                PopupMenuItem::new(delete_label)
                    .icon(Icon::new(IconName::Delete))
                    .on_click({
                        let weak = weak.clone();
                        let task_id = task_id.clone();
                        move |_, window, cx| {
                            let id = task_id.clone();
                            weak.update(cx, |this, cx| {
                                this.open_delete_confirm(id, false, window, cx);
                            })
                            .ok();
                        }
                    }),
            )
        }
    }

    pub(super) fn subtask_menu_builder(
        weak: WeakEntity<Self>,
        sub_id: String,
        sub_due: Option<DueDate>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        move |menu, _window, cx| {
            let add_due_label = i18n_content(cx, "add_due_date");
            let delete_label = i18n_content(cx, "delete");

            menu.item(
                PopupMenuItem::new(add_due_label)
                    .icon(Icon::new(IconName::Calendar))
                    .on_click({
                        let weak = weak.clone();
                        let sub_id = sub_id.clone();
                        move |_, window, cx| {
                            let id = sub_id.clone();
                            weak.update(cx, move |this, cx| {
                                this.due_picker_for = Some(id);
                                this.due_picker_calendar_state.update(cx, |state, cx| {
                                    let d = match sub_due {
                                        Some(d) => Date::Single(Some(d.date)),
                                        None => Date::Single(None),
                                    };
                                    state.set_date(d, window, cx);
                                });
                                this.due_picker_time_input.update(cx, |state, cx| {
                                    state.set_placeholder(
                                        i18n_content(cx, "due_time_placeholder"),
                                        window,
                                        cx,
                                    );
                                    let value = sub_due
                                        .and_then(|due| due.time)
                                        .map(due_time_label)
                                        .unwrap_or_default();
                                    state.set_value(value, window, cx);
                                });
                                cx.notify();
                            })
                            .ok();
                        }
                    }),
            )
            .separator()
            .item(
                PopupMenuItem::new(delete_label)
                    .icon(Icon::new(IconName::Delete))
                    .on_click({
                        let weak = weak.clone();
                        let sub_id = sub_id.clone();
                        move |_, window, cx| {
                            let id = sub_id.clone();
                            weak.update(cx, |this, cx| {
                                this.open_delete_confirm(id, true, window, cx);
                            })
                            .ok();
                        }
                    }),
            )
        }
    }

    pub(super) fn render_options_menu(cx: &mut Context<Self>, task: &Task) -> AnyElement {
        let tid_selected = task.id.clone();
        let weak = cx.entity().downgrade();

        Button::new(ElementId::Name(format!("task-menu-{}", task.id).into()))
            .icon(IconName::Ellipsis)
            .ghost()
            .small()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                if this.selected_task_id.as_deref() != Some(tid_selected.as_str()) {
                    this.selected_task_id = Some(tid_selected.clone());
                    this.selected_subtask_id = None;
                    cx.notify();
                }
            }))
            .dropdown_menu(Self::task_menu_builder(
                weak,
                task.id.clone(),
                task.due_date,
            ))
            .into_any_element()
    }

    pub(super) fn render_subtask_options_menu(
        cx: &mut Context<Self>,
        parent_id: &str,
        sub: &Task,
    ) -> AnyElement {
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
                    this.selected_task_id = None;
                    cx.notify();
                }
            }))
            .dropdown_menu(Self::subtask_menu_builder(
                weak,
                sub.id.clone(),
                sub.due_date,
            ))
            .into_any_element()
    }
}
