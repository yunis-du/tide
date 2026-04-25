use gpui::{App, Context, Window, div, prelude::*, px};
use gpui_component::{ActiveTheme, WindowExt, button::ButtonVariant, dialog::DialogButtonProps};

use crate::{
    helpers::i18n_content,
    state::{Task, TideDataStore, TideStore, tide::update_status, update_data_and_save},
};

use super::view::TaskView;

impl TaskView {
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

    pub(super) fn enter_task(this: &mut Self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn close_form(this: &mut Self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn enter_subtask(this: &mut Self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn open_add_subtask(
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

    pub(super) fn open_edit_subtask(
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

    pub(super) fn edit_selected_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn delete_selected_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(selected_sub_id) = self.selected_subtask_id.clone() {
            Self::open_delete_confirm(selected_sub_id, true, window, cx);
            return;
        }

        let Some(selected_id) = self.selected_task_id.clone() else {
            return;
        };
        Self::open_delete_confirm(selected_id, false, window, cx);
    }

    pub(super) fn open_delete_confirm(
        id: String,
        is_subtask: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
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
}
