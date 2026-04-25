use std::cell::RefCell;

use gpui::{
    App, Bounds, Context, ElementId, FontWeight, IntoElement, MouseButton, Render, Window,
    WindowBounds, WindowControlArea, WindowId, WindowKind, WindowOptions, div, prelude::*, px,
    rgba, size,
};
use gpui_component::{ActiveTheme, Icon, IconName, h_flex, scroll::ScrollableElement, v_flex};

use crate::{
    components::RadioButton,
    state::{Task, TideDataStore, update_data_and_save},
};

thread_local! {
    static FLOATING_WINDOWS: RefCell<Vec<WindowId>> = const { RefCell::new(Vec::new()) };
}

pub fn floating_window_ids() -> Vec<WindowId> {
    FLOATING_WINDOWS.with(|cell| cell.borrow().clone())
}

pub fn has_floating_windows() -> bool {
    FLOATING_WINDOWS.with(|cell| !cell.borrow().is_empty())
}

fn register_floating(id: WindowId) {
    FLOATING_WINDOWS.with(|cell| cell.borrow_mut().push(id));
}

fn unregister_floating(id: WindowId) {
    FLOATING_WINDOWS.with(|cell| cell.borrow_mut().retain(|x| *x != id));
}

pub struct FloatingGroupView {
    group_id: String,
    hovered_task_id: Option<String>,
}

impl FloatingGroupView {
    pub fn new(group_id: String) -> Self {
        Self {
            group_id,
            hovered_task_id: None,
        }
    }
}

impl Render for FloatingGroupView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bg = cx.theme().background;
        let fg = cx.theme().foreground;
        let muted_fg = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let title_bar_bg = cx.theme().title_bar;
        let list_even = cx.theme().list_even;

        let (group_name, pending) = {
            let data = cx.global::<TideDataStore>().read(cx);
            let name = data
                .task_groups()
                .iter()
                .find(|g| g.id == self.group_id)
                .map(|g| g.name.clone())
                .unwrap_or_default();
            let pending: Vec<Task> = data
                .tasks
                .iter()
                .filter(|t| t.parent_id.is_none() && t.group_id == self.group_id && !t.is_completed)
                .cloned()
                .collect();
            (name, pending)
        };

        let mut task_els = Vec::new();
        for task in &pending {
            let task_id = task.id.clone();
            let tid_check = task.id.clone();
            let tid_hover = task.id.clone();
            let title = task.title.clone();
            let is_hovered = self.hovered_task_id.as_deref() == Some(task.id.as_str());

            let row = h_flex()
                .id(ElementId::Name(format!("floating-task-{}", task_id).into()))
                .w_full()
                .px_2()
                .py_1p5()
                .gap_2()
                .items_center()
                .rounded_lg()
                .when(is_hovered, |s| s.bg(list_even))
                .hover(|s| s.bg(list_even))
                .on_hover(cx.listener(move |this, hov: &bool, _, cx| {
                    let next = if *hov { Some(tid_hover.clone()) } else { None };
                    if this.hovered_task_id != next {
                        this.hovered_task_id = next;
                        cx.notify();
                    }
                }))
                .child(RadioButton::new(task_id.clone()).on_click(move |_, _, cx| {
                    let id = tid_check.clone();
                    update_data_and_save(cx, "toggle_done", move |data, _| {
                        data.toggle_task_completion(&id);
                    });
                }))
                .child(div().flex_1().text_sm().text_color(fg).child(title));

            task_els.push(row);
        }

        let header = h_flex()
            .h(px(36.))
            .pl_3()
            .pr_1()
            .border_b_1()
            .border_color(border)
            .bg(title_bar_bg)
            .items_center()
            .gap_2()
            .child(
                h_flex()
                    .id("floating-drag")
                    .flex_1()
                    .h_full()
                    .items_center()
                    .window_control_area(WindowControlArea::Drag)
                    .on_mouse_down(MouseButton::Left, |_, window, _| {
                        window.start_window_move();
                    })
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight(600.))
                            .text_color(fg)
                            .child(group_name),
                    ),
            )
            .child(
                div()
                    .id("floating-close")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(24.))
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(muted_fg)
                    .hover(|s| s.bg(rgba(0x00000010)))
                    .on_click(|_, window, _| {
                        window.remove_window();
                    })
                    .child(Icon::new(IconName::Close).size_4()),
            );

        let empty_hint = if pending.is_empty() {
            Some(
                div()
                    .py_4()
                    .px_2()
                    .text_xs()
                    .text_color(muted_fg)
                    .child("—")
                    .into_any_element(),
            )
        } else {
            None
        };

        v_flex().size_full().bg(bg).child(header).child(
            v_flex()
                .id("floating-list")
                .flex_1()
                .min_h_0()
                .size_full()
                .px_2()
                .py_2()
                .gap_0p5()
                .overflow_y_scrollbar()
                .children(task_els)
                .children(empty_hint),
        )
    }
}

pub fn open_pinned_group_window(cx: &mut App, group_id: String) {
    let window_size = size(px(280.), px(420.));
    let bounds = Bounds::centered(None, window_size, cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        is_resizable: true,
        is_movable: true,
        is_minimizable: false,
        kind: WindowKind::PopUp,
        focus: true,
        show: true,
        window_min_size: Some(size(px(220.), px(200.))),
        ..Default::default()
    };

    let opened = cx.open_window(options, |window, cx| {
        let id = window.window_handle().window_id();
        register_floating(id);
        // On Windows, `WindowKind::PopUp` doesn't imply WS_EX_TOPMOST, so we
        // promote the HWND to topmost ourselves. No-op on other platforms.
        crate::set_window_always_on_top(window);
        window.on_window_should_close(cx, move |_, _| {
            unregister_floating(id);
            true
        });
        cx.new(|_cx| FloatingGroupView::new(group_id.clone()))
    });

    if let Err(e) = opened {
        tracing::warn!(error = %e, "failed to open pinned group window");
    }
}
