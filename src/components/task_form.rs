use std::rc::Rc;

use chrono::{Local, NaiveDate};
use gpui::{
    App, Corner, ElementId, Entity, Hsla, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, RenderOnce, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px, rgba,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    calendar::{Calendar, CalendarState},
    h_flex,
    input::{Input, InputState},
    popover::Popover,
    v_flex,
};

use crate::{
    helpers::i18n_content,
    state::{TideStore, tide::update_status},
};

type DueDateHandler = Rc<dyn Fn(&Option<NaiveDate>, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct TaskForm {
    title_input: Entity<InputState>,
    details_input: Entity<InputState>,
    pending_due_date: Option<NaiveDate>,
    calendar_state: Option<Entity<CalendarState>>,
    on_set_due_date: Option<DueDateHandler>,
    on_mouse_down_out: Option<Rc<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>>,
}

impl TaskForm {
    pub fn new(title_input: Entity<InputState>, details_input: Entity<InputState>) -> Self {
        Self {
            title_input,
            details_input,
            pending_due_date: None,
            calendar_state: None,
            on_set_due_date: None,
            on_mouse_down_out: None,
        }
    }

    pub fn pending_due_date(mut self, date: Option<NaiveDate>) -> Self {
        self.pending_due_date = date;
        self
    }

    pub fn calendar_state(mut self, state: Entity<CalendarState>) -> Self {
        self.calendar_state = Some(state);
        self
    }

    pub fn on_set_due_date(
        mut self,
        handler: impl Fn(&Option<NaiveDate>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_set_due_date = Some(Rc::new(handler));
        self
    }

    pub fn on_mouse_down_out(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_mouse_down_out = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for TaskForm {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let title_ph = i18n_content(cx, "title_placeholder");
        let details_ph = i18n_content(cx, "details_placeholder");
        self.title_input.update(cx, |state, cx| {
            state.set_placeholder(title_ph, window, cx);
        });
        self.details_input.update(cx, |state, cx| {
            state.set_placeholder(details_ph, window, cx);
        });

        let muted_fg = cx.theme().muted_foreground;
        let border = cx.theme().border;

        let today_label = i18n_content(cx, "today");
        let tomorrow_label = i18n_content(cx, "tomorrow");
        let mouse_out = self.on_mouse_down_out;
        let set_due = self.on_set_due_date;
        let pending = self.pending_due_date;
        let calendar_state = self.calendar_state;
        let is_calendar_open = cx
            .global::<TideStore>()
            .read(cx)
            .status()
            .task_calendar_open();

        let today_handler = set_due.clone();
        let tomorrow_handler = set_due.clone();
        let clear_handler = set_due.clone();

        let row = h_flex().pl(px(32.)).gap_2().items_center();

        let date_row = if let Some(date) = pending {
            row.child(
                super::DateTag::new(date)
                    .removable()
                    .on_remove(move |window, cx| {
                        if let Some(h) = clear_handler.as_ref() {
                            h(&None, window, cx);
                        }
                    }),
            )
        } else {
            let cal_trigger = Button::new("form-cal")
                .icon(IconName::Calendar)
                .ghost()
                .small()
                .cursor_pointer();

            row.child(pill_btn(
                "form-today",
                today_label,
                border,
                move |window, cx| {
                    if let Some(h) = today_handler.as_ref() {
                        h(&Some(Local::now().date_naive()), window, cx);
                    }
                },
            ))
            .child(pill_btn(
                "form-tomorrow",
                tomorrow_label,
                border,
                move |window, cx| {
                    if let Some(h) = tomorrow_handler.as_ref() {
                        let tomorrow = Local::now().date_naive() + chrono::Duration::days(1);
                        h(&Some(tomorrow), window, cx);
                    }
                },
            ))
            .child(calendar_popover(
                "form-cal-popover",
                cal_trigger,
                calendar_state.clone(),
            ))
        };

        v_flex()
            .w_full()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .gap_1()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .when_some(mouse_out, |this, h| {
                this.on_mouse_down_out(move |ev, window, cx| {
                    if !is_calendar_open {
                        h(ev, window, cx);
                    }
                })
            })
            .child(
                h_flex()
                    .gap_3()
                    .items_start()
                    .child(
                        div()
                            .mt_1()
                            .size_5()
                            .rounded_full()
                            .flex_shrink_0()
                            .border_2()
                            .border_color(border),
                    )
                    .child(Input::new(&self.title_input).appearance(false).flex_1()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .pl(px(32.))
                    .child(Icon::new(IconName::Menu).size_4().text_color(muted_fg))
                    .child(Input::new(&self.details_input).appearance(false).flex_1()),
            )
            .child(date_row)
    }
}

fn pill_btn(
    id: &'static str,
    label: String,
    border: Hsla,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(ElementId::Name(id.into()))
        .px_3()
        .py_1()
        .rounded_full()
        .border_1()
        .border_color(border)
        .text_xs()
        .cursor_pointer()
        .hover(|s| s.bg(rgba(0x00000010)))
        .child(label)
        .on_click(move |_, window, cx| {
            on_click(window, cx);
        })
}

fn calendar_popover(
    id: &'static str,
    trigger: Button,
    calendar_state: Option<Entity<CalendarState>>,
) -> gpui::AnyElement {
    let Some(cal) = calendar_state else {
        return trigger.into_any_element();
    };

    let popover = Popover::new(id)
        .anchor(Corner::TopLeft)
        .trigger(trigger)
        .content(move |_, _, _| div().p_2().child(Calendar::new(&cal).number_of_months(1)));

    let popover = popover.on_open_change(move |open, _, cx| {
        let is_open = *open;
        update_status(cx, move |status, _| {
            status.set_task_calendar_open(is_open);
        });
    });

    popover.into_any_element()
}
