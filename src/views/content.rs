use gpui::{Context, Entity, FontWeight, IntoElement, Render, Window, div, prelude::*};
use gpui_component::{ActiveTheme, v_flex};

use crate::{
    helpers::i18n_content,
    state::{SidebarSelection, TideDataStore},
};

use super::{SettingsView, content::view::TaskView};

mod actions;
mod drag;
mod menu;
mod render;
mod row_completed;
mod row_pending;
mod view;

pub struct ContentView {
    task_view: Entity<TaskView>,
    settings_view: Entity<SettingsView>,
}

impl ContentView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let task_view = cx.new(|cx| TaskView::new(window, cx));
        let settings_view = cx.new(|cx| SettingsView::new(window, cx));
        Self {
            task_view,
            settings_view,
        }
    }
}

impl Render for ContentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let data = cx.global::<TideDataStore>().read(cx);

        let sel = data.sidebar_selection().clone();
        if sel == SidebarSelection::Settings {
            return self.settings_view.clone().into_any_element();
        }

        let group_name = match &sel {
            SidebarSelection::AllTasks => i18n_content(cx, "all_tasks"),
            SidebarSelection::Starred => i18n_content(cx, "starred"),
            SidebarSelection::Settings => unreachable!(),
            SidebarSelection::Group(id) => data
                .task_groups()
                .iter()
                .find(|l| &l.id == id)
                .map(|l| l.name.clone())
                .unwrap_or_default(),
        };

        let fg = cx.theme().foreground;
        let background = cx.theme().background;

        v_flex()
            .flex_1()
            .h_full()
            .bg(background)
            .overflow_hidden()
            .child(
                div()
                    .px_6()
                    .pt_6()
                    .pb_2()
                    .items_center()
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .text_color(fg)
                    .child(group_name),
            )
            .child(
                v_flex()
                    .id("task-scroll")
                    .flex_1()
                    .min_h_0()
                    .px_4()
                    .pb_4()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .child(self.task_view.clone()),
                    ),
            )
            .into_any_element()
    }
}
