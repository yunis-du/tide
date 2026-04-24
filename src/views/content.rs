use gpui::{Context, Entity, FontWeight, IntoElement, Render, Window, div, prelude::*};
use gpui_component::{ActiveTheme, v_flex};

use crate::{
    helpers::i18n_content,
    state::{SidebarSelection, TideDataStore},
};

use super::content::task::TaskView;

mod task;

pub struct ContentView {
    task_view: Entity<TaskView>,
}

impl ContentView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let task_view = cx.new(|cx| TaskView::new(window, cx));
        Self { task_view }
    }
}

impl Render for ContentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let data = cx.global::<TideDataStore>().read(cx);

        let sel = data.sidebar_selection().clone();
        let group_name = match &sel {
            SidebarSelection::AllTasks => i18n_content(cx, "all_tasks"),
            SidebarSelection::Starred => i18n_content(cx, "starred"),
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
    }
}
