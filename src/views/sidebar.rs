use gpui::{
    AnyElement, Context, Corner, ElementId, Entity, FontWeight, IntoElement, MouseDownEvent,
    Render, Subscription, Window, div, prelude::*, px, rgba,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariant, ButtonVariants},
    dialog::DialogButtonProps,
    h_flex,
    input::{Escape, Input, InputEvent, InputState},
    menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
    scroll::ScrollableElement,
    v_flex,
};

use crate::{
    helpers::{active_item_bg, i18n_sidebar, interactive_accent},
    state::{
        SidebarSelection, TaskGroup, TideDataStore, TideStore, tide::update_status,
        update_data_and_save,
    },
};

use super::floating::open_pinned_group_window;

pub struct SidebarView {
    hovered_group_id: Option<String>,
    group_input: Entity<InputState>,
    new_group: Option<TaskGroup>,

    _subs: Vec<Subscription>,
}

impl SidebarView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let group_input = cx.new(|cx| InputState::new(window, cx));

        let mut subs = Vec::new();

        let inp = group_input.clone();
        subs.push(cx.subscribe_in(
            &inp,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => Self::enter_group_name(this, window, cx),
                InputEvent::Blur => Self::cancel_group_name(this, window, cx),
                _ => {}
            },
        ));

        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                Self::enter_group_name(this, window, cx);
            }
        })
        .detach();

        Self {
            hovered_group_id: None,
            group_input,
            new_group: None,
            _subs: subs,
        }
    }

    /// Commit rename if value is non-empty, otherwise cancel.
    fn enter_group_name(this: &mut Self, window: &mut Window, cx: &mut Context<Self>) {
        let edit_group_id = cx.global::<TideStore>().read(cx).status().edit_group_id();

        let name = this.group_input.read(cx).value().to_string();
        let trimmed = name.trim().to_string();

        if !trimmed.is_empty() {
            if this.new_group.is_some() {
                // create new group
                let Some(mut new_group) = this.new_group.clone() else {
                    return;
                };
                new_group.name = trimmed;
                update_data_and_save(cx, "create_group", move |data, _| {
                    data.add_task_group(new_group);
                });
                // update status
                update_status(cx, move |status, _| {
                    status.set_edit_group_id(None);
                    status.set_create_group(false);
                });
                this.new_group = None;
            } else if edit_group_id.is_some() {
                // rename existing group
                let Some(id) = edit_group_id else {
                    return;
                };
                update_data_and_save(cx, "rename_group", move |data, cx| {
                    data.rename_task_group(&id, trimmed.clone());

                    // update status
                    update_status(cx, move |status, _| {
                        status.set_edit_group_id(None);
                    });
                });
            }
        } else {
            update_status(cx, move |status, _| {
                status.set_edit_group_id(None);
                status.set_create_group(false);
            });
        }

        this.group_input.update(cx, |inp, cx| {
            inp.set_value("", window, cx);
        });
        cx.notify();
    }

    fn cancel_group_name(this: &mut Self, window: &mut Window, cx: &mut Context<Self>) {
        let edit_group_id = cx.global::<TideStore>().read(cx).status().edit_group_id();

        if this.new_group.is_some() {
            // cancel create new group
            this.new_group = None;

            // update status
            update_status(cx, move |status, _| {
                status.set_create_group(false);
            });
        } else if edit_group_id.is_some() {
            // cancel rename existing group
            let Some(id) = edit_group_id else {
                return;
            };
            update_data_and_save(cx, "cancel_rename", move |data, cx| {
                let is_new_empty = data
                    .task_groups()
                    .iter()
                    .any(|g| g.id == id && g.name.is_empty());
                if is_new_empty {
                    data.remove_task_group(&id);
                }
                // update status
                update_status(cx, move |status, _| {
                    status.set_edit_group_id(None);
                });
            });
        }

        this.group_input.update(cx, |inp, cx| {
            inp.set_value("", window, cx);
        });
        cx.notify();
    }

    fn render_nav_row(
        cx: &mut Context<Self>,
        id: &'static str,
        label: String,
        icon: IconName,
        is_active: bool,
        sel: SidebarSelection,
    ) -> impl IntoElement {
        let accent = interactive_accent(cx.theme());
        let muted_fg = cx.theme().muted_foreground;
        let fg = cx.theme().foreground;

        let icon_color = if is_active { accent } else { muted_fg };
        let text_color = if is_active { accent } else { fg };
        let bg = if is_active {
            Some(active_item_bg(cx.theme()))
        } else {
            None
        };

        h_flex()
            .id(ElementId::Name(id.into()))
            .w_full()
            .rounded_lg()
            .px_2()
            .py_1p5()
            .gap_2()
            .cursor_pointer()
            .when_some(bg, |t, c| t.bg(c))
            .hover(|s| s.bg(rgba(0x00000010)))
            .on_click(move |_, _, cx| {
                let s = sel.clone();
                update_data_and_save(cx, "set_selection", move |data, _| {
                    data.set_sidebar_selection(s.clone());
                });
            })
            .child(Icon::new(icon).size_4().text_color(icon_color))
            .child(div().text_sm().text_color(text_color).child(label))
    }

    fn render_settings_row(cx: &mut Context<Self>, is_active: bool) -> impl IntoElement {
        let accent = interactive_accent(cx.theme());
        let fg = cx.theme().foreground;
        let icon_color = if is_active { accent } else { fg };
        let text_color = if is_active { accent } else { fg };
        let bg = if is_active {
            Some(active_item_bg(cx.theme()))
        } else {
            None
        };

        h_flex()
            .id("nav-settings")
            .w_full()
            .rounded_lg()
            .px_2()
            .py_2()
            .gap_3()
            .items_center()
            .cursor_pointer()
            .when_some(bg, |t, c| t.bg(c))
            .hover(|s| s.bg(rgba(0x00000010)))
            .on_click(move |_, _, cx| {
                update_data_and_save(cx, "set_selection", move |data, _| {
                    data.set_sidebar_selection(SidebarSelection::Settings);
                });
            })
            .child(
                Icon::new(IconName::Settings)
                    .size_5()
                    .text_color(icon_color),
            )
            .child(
                div()
                    .text_base()
                    .text_color(text_color)
                    .child(i18n_sidebar(cx, "settings")),
            )
    }

    fn group_menu_builder(
        id: String,
        group_name: String,
        inp: Entity<InputState>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        move |menu, _window, cx| {
            let rename_label = i18n_sidebar(cx, "rename");
            let pin_label = i18n_sidebar(cx, "pin_to_desktop");
            let delete_label = i18n_sidebar(cx, "delete_group");

            menu.item(PopupMenuItem::new(rename_label).on_click({
                let value = inp.clone();
                let group_name = group_name.clone();
                let id = id.clone();
                move |_, window, cx| {
                    let id_for_state = id.clone();
                    value.update(cx, |state, cx| {
                        state.set_value(&group_name, window, cx);
                        state.focus(window, cx);
                    });

                    update_status(cx, move |status, _| {
                        status.set_edit_group_id(Some(id_for_state));
                    });
                }
            }))
            .item(
                PopupMenuItem::new(pin_label)
                    .icon(Icon::new(IconName::ExternalLink))
                    .on_click({
                        let id = id.clone();
                        move |_, _, cx| {
                            open_pinned_group_window(cx, id.clone());
                        }
                    }),
            )
            .separator()
            .item(PopupMenuItem::new(delete_label).on_click({
                let id = id.clone();
                move |_, window, cx| {
                    let id = id.clone();
                    window.open_dialog(cx, move |dialog, window, cx| {
                        let id_for_del = id.clone();
                        let dialog_width = px(360.);
                        let dialog_height = px(160.);
                        let margin_top =
                            ((window.viewport_size().height - dialog_height) / 2.).max(px(0.));
                        dialog
                            .title(i18n_sidebar(cx, "delete_group_title"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(i18n_sidebar(cx, "delete_group_desc")),
                            )
                            .w(dialog_width)
                            .margin_top(margin_top)
                            .confirm()
                            .button_props(
                                DialogButtonProps::default()
                                    .ok_text(i18n_sidebar(cx, "confirm_delete"))
                                    .cancel_text(i18n_sidebar(cx, "cancel"))
                                    .ok_variant(ButtonVariant::Danger),
                            )
                            .on_ok(move |_, _, cx| {
                                let id = id_for_del.clone();
                                update_data_and_save(cx, "delete_group", move |data, _| {
                                    data.remove_task_group(&id);
                                });
                                true
                            })
                    });
                }
            }))
        }
    }

    fn render_options_menu(id: &str, group_name: &str, inp: Entity<InputState>) -> AnyElement {
        Button::new(ElementId::Name(format!("group-menu-{}", id).into()))
            .icon(IconName::Ellipsis)
            .ghost()
            .small()
            .cursor_pointer()
            .dropdown_menu(Self::group_menu_builder(
                id.to_string(),
                group_name.to_string(),
                inp,
            ))
            .anchor(Corner::TopRight)
            .into_any_element()
    }

    fn render_create_group_btn(
        cx: &mut Context<Self>,
        group_input: Entity<InputState>,
    ) -> AnyElement {
        let muted_fg = cx.theme().muted_foreground;

        h_flex()
            .id("new-group-btn")
            .w_full()
            .rounded_lg()
            .px_2()
            .py_1p5()
            .gap_2()
            .cursor_pointer()
            .hover(|s| s.bg(rgba(0x00000010)))
            .on_click(cx.listener(move |this, _, window, cx| {
                let new_group = TaskGroup::new("");
                let new_group_id = new_group.id.clone();
                this.new_group = Some(new_group);

                group_input.update(cx, |inp, cx| {
                    inp.set_value("", window, cx);
                    inp.focus(window, cx);
                });

                update_status(cx, move |status, _| {
                    status.set_edit_group_id(Some(new_group_id));
                    status.set_create_group(true);
                });
            }))
            .child(Icon::new(IconName::Plus).size_4().text_color(muted_fg))
            .child(
                div()
                    .text_sm()
                    .text_color(muted_fg)
                    .child(i18n_sidebar(cx, "new_group")),
            )
            .into_any_element()
    }

    fn render_group_header(
        cx: &mut Context<Self>,
        group_input: Entity<InputState>,
        is_create_group: bool,
    ) -> AnyElement {
        let muted_fg = cx.theme().muted_foreground;

        let groups_label = h_flex().w_full().px_2().py_1().justify_between().child(
            div()
                .text_xs()
                .font_weight(FontWeight(500.))
                .text_color(muted_fg)
                .child(i18n_sidebar(cx, "task_groups")),
        );

        v_flex()
            .mt_4()
            .px_2()
            .gap_1()
            .child(groups_label)
            .when(!is_create_group, |t| {
                t.child(Self::render_create_group_btn(cx, group_input))
            })
            .into_any_element()
    }

    fn render_group_list(
        cx: &mut Context<Self>,
        hovered_group: Option<String>,
        group_input: Entity<InputState>,
        new_group: Option<TaskGroup>,
    ) -> AnyElement {
        let (selection, mut groups, editing_group_id, is_create_group) = {
            let data = cx.global::<TideDataStore>().read(cx);
            let status = cx.global::<TideStore>().read(cx).status();
            (
                data.sidebar_selection().clone(),
                data.task_groups().to_vec(),
                status.edit_group_id(),
                status.create_group(),
            )
        };

        if new_group.is_some() && is_create_group {
            groups.push(new_group.clone().unwrap());
        }
        groups.reverse();

        let active_color = interactive_accent(cx.theme());
        let fg = cx.theme().foreground;
        let muted_fg = cx.theme().muted_foreground;

        let mut group_els: Vec<AnyElement> = Vec::new();
        for group in &groups {
            let is_active = selection == SidebarSelection::Group(group.id.clone());
            let is_editing = editing_group_id.as_deref() == Some(group.id.as_str());
            let is_hovered = hovered_group.as_deref() == Some(group.id.as_str());
            let group_id = group.id.clone();
            let group_id_hover = group.id.clone();
            let group_name = group.name.clone();
            let fg_color = if is_active { active_color } else { fg };
            let icon_color = if is_active { active_color } else { muted_fg };
            let bg_color = if is_active {
                Some(active_item_bg(cx.theme()))
            } else {
                None
            };

            let editing_id_for_click = editing_group_id.clone();
            let group_id_for_menu = group.id.clone();
            let group_name_for_menu = group.name.clone();
            let group_input_for_menu = group_input.clone();

            let group_row = h_flex()
                .id(ElementId::Name(format!("group-{}", group.id).into()))
                .w_full()
                .rounded_lg()
                .px_2()
                .py_1p5()
                .gap_2()
                .cursor_pointer()
                .when_some(bg_color, |t, c| t.bg(c))
                .hover(|s| s.bg(rgba(0x00000010)))
                .on_hover(cx.listener(move |this, is_hov: &bool, _, cx| {
                    let new_val = if *is_hov {
                        Some(group_id_hover.clone())
                    } else {
                        None
                    };
                    if this.hovered_group_id != new_val {
                        this.hovered_group_id = new_val;
                        cx.notify();
                    }
                }))
                .on_click(move |_, _, cx| {
                    if editing_id_for_click.is_none() {
                        let id = group_id.clone();
                        update_data_and_save(cx, "select_group", move |data, _| {
                            data.set_sidebar_selection(SidebarSelection::Group(id.clone()));
                        });
                    }
                })
                .child(
                    Icon::new(IconName::CircleCheck)
                        .size_4()
                        .text_color(icon_color),
                )
                .when(is_editing, |t| {
                    t.child(
                        div()
                            .flex_1()
                            .child(
                                Input::new(&group_input)
                                    .appearance(false)
                                    .flex_1()
                                    .text_color(fg_color),
                            )
                            .on_mouse_down_out(cx.listener(
                                |this, _: &MouseDownEvent, window, cx| {
                                    Self::enter_group_name(this, window, cx);
                                },
                            )),
                    )
                })
                .when(!is_editing, |t| {
                    t.child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(fg_color)
                            .child(group_name),
                    )
                    .child(
                        div()
                            .opacity(if is_hovered { 1.0 } else { 0.0 })
                            .when(!is_hovered, |d| d.cursor_default())
                            .child(Self::render_options_menu(
                                group.id.as_str(),
                                group.name.as_str(),
                                group_input.clone(),
                            )),
                    )
                })
                .context_menu(Self::group_menu_builder(
                    group_id_for_menu,
                    group_name_for_menu,
                    group_input_for_menu,
                ));

            group_els.push(v_flex().w_full().child(group_row).into_any_element());
        }

        v_flex()
            .px_2()
            .gap_1()
            .children(group_els)
            .into_any_element()
    }
}

impl Render for SidebarView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selection = {
            let data = cx.global::<TideDataStore>().read(cx);
            data.sidebar_selection().clone()
        };

        let sidebar_bg = cx.theme().sidebar;
        let border_color = cx.theme().sidebar_border;

        let all_active = selection == SidebarSelection::AllTasks;
        let star_active = selection == SidebarSelection::Starred;
        let settings_active = selection == SidebarSelection::Settings;

        let all_label = i18n_sidebar(cx, "all_tasks");
        let star_label = i18n_sidebar(cx, "starred");

        let hovered_group = self.hovered_group_id.clone();
        let group_input = self.group_input.clone();
        let is_create_group = cx.global::<TideStore>().read(cx).status().create_group();

        let nav_rows = {
            v_flex()
                .px_2()
                .gap_1()
                .child(Self::render_nav_row(
                    cx,
                    "nav-all",
                    all_label,
                    IconName::CircleCheck,
                    all_active,
                    SidebarSelection::AllTasks,
                ))
                .child(Self::render_nav_row(
                    cx,
                    "nav-star",
                    star_label,
                    IconName::Star,
                    star_active,
                    SidebarSelection::Starred,
                ))
        };

        v_flex()
            .p_4()
            .w(px(240.))
            .h_full()
            .flex_shrink_0()
            .bg(sidebar_bg)
            .border_r_1()
            .border_color(border_color)
            .on_action(cx.listener(move |_, _: &Escape, _, cx| {
                update_status(cx, move |status, _| {
                    status.set_edit_group_id(None);
                    status.set_create_group(false);
                });
            }))
            .child(nav_rows)
            .child(Self::render_group_header(
                cx,
                group_input.clone(),
                is_create_group,
            ))
            .child(
                div().flex_1().min_h_0().overflow_hidden().child(
                    v_flex()
                        .id("sidebar-groups-scroll")
                        .size_full()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .child(Self::render_group_list(
                            cx,
                            hovered_group,
                            group_input,
                            self.new_group.clone(),
                        )),
                ),
            )
            .child(
                div()
                    .px_2()
                    .pt_2()
                    .child(Self::render_settings_row(cx, settings_active)),
            )
    }
}
