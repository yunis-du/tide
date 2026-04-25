use gpui::{Context, IntoElement, Render, Styled, Window, div, prelude::*, px};
use gpui_component::{ActiveTheme, h_flex};

#[derive(Clone)]
pub(super) struct DragTask {
    pub(super) id: String,
    pub(super) title: String,
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
pub(super) struct DragSubTask {
    pub(super) id: String,
    pub(super) parent_id: String,
    pub(super) title: String,
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
