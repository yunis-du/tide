use std::rc::Rc;

use chrono::NaiveDate;
use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, prelude::FluentBuilder};
use gpui_component::{
    Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
};

use crate::helpers::{due_date_color, due_date_label};

#[derive(IntoElement)]
pub struct DateTag {
    date: NaiveDate,
    removable: bool,
    on_remove: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
}

impl DateTag {
    pub fn new(date: NaiveDate) -> Self {
        Self {
            date,
            removable: false,
            on_remove: None,
        }
    }

    pub fn on_remove(mut self, on_remove: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_remove = Some(Rc::new(on_remove));
        self
    }

    pub fn removable(mut self) -> Self {
        self.removable = true;
        self
    }
}

impl RenderOnce for DateTag {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = due_date_color(cx, self.date);
        let label = due_date_label(cx, self.date);

        h_flex()
            .gap_1()
            .px_2()
            .py_0p5()
            .rounded_full()
            .border_1()
            .border_color(color)
            .text_xs()
            .text_color(color)
            .child(Icon::new(IconName::Calendar).size_3().text_color(color))
            .child(label)
            .when(self.removable, |this| {
                this.child(
                    Button::new("tag-remove")
                        .icon(IconName::Close)
                        .ghost()
                        .xsmall()
                        .cursor_pointer()
                        .text_color(color)
                        .on_click(move |_, window, cx| {
                            if let Some(h) = self.on_remove.as_ref() {
                                h(window, cx);
                            }
                        }),
                )
            })
    }
}
