use std::rc::Rc;

use gpui::{
    App, ElementId, IntoElement, ParentElement, RenderOnce, Styled, Window, prelude::FluentBuilder,
};
use gpui_component::{
    Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
};

use crate::{
    helpers::{due_date_color, due_date_label},
    state::DueDate,
};

#[derive(IntoElement)]
pub struct DateTag {
    due: DueDate,
    removable: bool,
    remove_id: Option<ElementId>,
    on_remove: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
}

impl DateTag {
    pub fn new(due: DueDate) -> Self {
        Self {
            due,
            removable: false,
            remove_id: None,
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

    pub fn remove_id(mut self, id: impl Into<ElementId>) -> Self {
        self.remove_id = Some(id.into());
        self
    }
}

impl RenderOnce for DateTag {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = due_date_color(cx, self.due);
        let label = due_date_label(cx, self.due);
        let remove_id = self.remove_id.unwrap_or_else(|| "tag-remove".into());

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
                    Button::new(remove_id)
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
