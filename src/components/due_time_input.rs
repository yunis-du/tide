use std::rc::Rc;

use gpui::{
    App, Entity, InteractiveElement, IntoElement, MouseButton, ParentElement, RenderOnce, Styled,
    Window, div, prelude::FluentBuilder, px, rgba,
};
use gpui_component::{
    ActiveTheme, Icon,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    scroll::ScrollableElement,
    v_flex,
};

use crate::assets::CustomIconName;

const MENU_MAX_HEIGHT: f32 = 220.;
type TimeSelectHandler = Rc<dyn Fn(&str, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct DueTimeInput {
    id: &'static str,
    input: Entity<InputState>,
    on_select: Option<TimeSelectHandler>,
}

impl DueTimeInput {
    pub fn new(id: &'static str, input: Entity<InputState>) -> Self {
        Self {
            id,
            input,
            on_select: None,
        }
    }

    pub fn on_select(mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for DueTimeInput {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let border = cx.theme().border;
        let muted_fg = cx.theme().muted_foreground;
        let selected = self.input.read(cx).value().to_string();
        let open_state = window.use_keyed_state(format!("{}-open", self.id), cx, |_, _| false);
        let is_open = *open_state.read(cx);

        v_flex()
            .w(px(180.))
            .gap_1()
            .child(
                Button::new(self.id)
                    .ghost()
                    .compact()
                    .w_full()
                    .h(px(36.))
                    .child(
                        h_flex()
                            .w_full()
                            .h_full()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .rounded_md()
                            .border_1()
                            .border_color(border)
                            .bg(cx.theme().secondary)
                            .child(
                                Icon::new(CustomIconName::Clock)
                                    .size_4()
                                    .text_color(muted_fg),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&self.input).appearance(false).w_full()),
                            )
                            .child(
                                Icon::new(gpui_component::IconName::ChevronDown)
                                    .size_3()
                                    .text_color(muted_fg),
                            ),
                    )
                    .on_click({
                        let open_state = open_state.clone();
                        move |_, _, cx| {
                            open_state.update(cx, |open, cx| {
                                *open = !*open;
                                cx.notify();
                            });
                        }
                    }),
            )
            .when(is_open, |this| {
                this.child(
                    v_flex()
                        .id(format!("{}-options", self.id))
                        .h(px(MENU_MAX_HEIGHT))
                        .rounded_md()
                        .border_1()
                        .border_color(border)
                        .bg(cx.theme().popover)
                        .overflow_y_scrollbar()
                        .children(time_options().into_iter().map(|label| {
                            let is_selected = selected.trim() == label;
                            let value = label.clone();
                            let input = self.input.clone();
                            let open_state = open_state.clone();
                            let on_select = self.on_select.clone();

                            div()
                                .id(format!("{}-option-{label}", self.id))
                                .px_3()
                                .py_1()
                                .text_sm()
                                .cursor_pointer()
                                .text_color(cx.theme().popover_foreground)
                                .when(is_selected, |this| this.bg(cx.theme().list_active))
                                .hover(|this| this.bg(rgba(0x00000010)))
                                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                    cx.stop_propagation();
                                    input.update(cx, |state, cx| {
                                        state.set_value(value.clone(), window, cx);
                                    });
                                    if let Some(handler) = on_select.as_ref() {
                                        handler(value.as_str(), window, cx);
                                    }
                                    open_state.update(cx, |open, cx| {
                                        *open = false;
                                        cx.notify();
                                    });
                                })
                                .child(label)
                        })),
                )
            })
    }
}

fn time_options() -> Vec<String> {
    (0..24)
        .flat_map(|hour| [format!("{hour:02}:00"), format!("{hour:02}:30")])
        .collect()
}
