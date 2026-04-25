use chrono::{Datelike, NaiveDate};
use gpui::{AnyElement, Context, ElementId, IntoElement, Styled, div, prelude::*, px, white};
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use rust_i18n::t;

use crate::{
    assets::CustomIconName,
    helpers::{i18n_content, locale, weekday_label},
    state::update_data_and_save,
};

use super::view::TaskView;

impl TaskView {
    pub(super) fn render_completed_row(
        &self,
        cx: &mut Context<Self>,
        id: String,
        title: String,
        completed_at: Option<NaiveDate>,
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
                    .when_some(completed_at, |t, date: NaiveDate| {
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
}
