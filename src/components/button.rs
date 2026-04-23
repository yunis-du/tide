use std::rc::Rc;

use gpui::{
    App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
    rems, transparent_black,
};
use gpui_component::{ActiveTheme, Icon, Sizable, Size, tooltip::Tooltip};

use crate::{assets::CustomIconName, helpers::i18n_content};

#[derive(IntoElement)]
pub struct RadioButton {
    id: String,
    size: Size,
    handler: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl RadioButton {
    pub fn new(id: String) -> Self {
        Self {
            id,
            size: Size::default(),
            handler: None,
        }
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.handler = Some(Rc::new(handler));
        self
    }
}

impl Sizable for RadioButton {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl RenderOnce for RadioButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let id_str: SharedString = format!("radio-btn-{}", self.id).into();
        let group_name = id_str.clone();
        let handler = self.handler;

        div()
            .id(ElementId::Name(id_str))
            .group(group_name.clone())
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .flex_none()
            .size(px(20.))
            .rounded_full()
            .border_1()
            .border_color(cx.theme().border)
            .map(|this| match self.size {
                Size::XSmall => this.size_3(),
                Size::Small => this.size_3p5(),
                Size::Medium => this.size_4(),
                Size::Large => this.size(rems(1.125)),
                _ => this.size_4(),
            })
            .tooltip(|window, cx| {
                Tooltip::new(i18n_content(cx, "marked_as_completed")).build(window, cx)
            })
            .hover(|s| {
                s.border_color(transparent_black())
                    .bg(cx.theme().sidebar_accent)
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .invisible()
                    .text_color(cx.theme().info_active)
                    .group_hover(group_name, |s| s.visible())
                    .child(
                        Icon::new(CustomIconName::Check).map(|this| match self.size {
                            Size::XSmall => this.size_3(),
                            Size::Small => this.size_3p5(),
                            Size::Medium => this.size_4(),
                            Size::Large => this.size(rems(1.125)),
                            _ => this,
                        }),
                    ),
            )
            .when_some(handler, |this, h| {
                this.on_click(move |event, window, cx| h(event, window, cx))
            })
    }
}
