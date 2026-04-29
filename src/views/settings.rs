use gpui::{
    AnyElement, App, ClickEvent, Context, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window, WindowAppearance, div, px,
    rgba,
};
use gpui_component::{
    ActiveTheme, Sizable, Theme, ThemeMode, h_flex, scroll::ScrollableElement, switch::Switch,
    v_flex,
};
use tracing::error;

use crate::{
    autostart,
    helpers::i18n_settings,
    state::{CloseBehavior, DefaultView, TideStore, update_and_save},
    tray,
};

pub struct SettingsView;

impl SettingsView {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }

    fn render_row(
        &self,
        cx: &mut Context<Self>,
        title: String,
        desc: String,
        control: impl IntoElement,
    ) -> impl IntoElement {
        h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .gap_4()
            .py_4()
            .child(
                v_flex()
                    .gap_1()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight(500.))
                            .text_color(cx.theme().foreground)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(desc),
                    ),
            )
            .child(control)
    }

    fn render_card(
        &self,
        title: String,
        border_color: Hsla,
        text_color: Hsla,
        children: impl IntoIterator<Item = AnyElement>,
    ) -> AnyElement {
        v_flex()
            .max_w(px(680.))
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(border_color)
            .bg(rgba(0xffffff08))
            .px_4()
            .pb_1()
            .child(
                div()
                    .pt_4()
                    .pb_2()
                    .text_sm()
                    .font_weight(FontWeight(600.))
                    .text_color(text_color)
                    .child(title),
            )
            .children(children)
            .into_any_element()
    }

    fn option_button(
        id: &'static str,
        label: String,
        selected: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> AnyElement {
        h_flex()
            .id(id)
            .px_2()
            .py_1()
            .rounded_lg()
            .cursor_pointer()
            .border_1()
            .text_sm()
            .border_color(rgba(if selected { 0x0088ccff } else { 0x00000020 }))
            .bg(rgba(if selected { 0x0088cc18 } else { 0x00000000 }))
            .hover(|s| s.bg(rgba(0x00000010)))
            .on_click(on_click)
            .child(label)
            .into_any_element()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let config = cx.global::<TideStore>().read(cx);
        let launch_at_login = config.launch_at_login();
        let show_main_window_on_startup = config.show_main_window_on_startup();
        let close_behavior = config.close_behavior();
        let default_view = config.default_view();
        let completed_expanded_by_default = config.completed_expanded_by_default();
        let locale = config.locale().to_string();
        let theme = config.theme();
        let auto_check_updates = config.auto_check_updates();
        let border_color = cx.theme().border;
        let fg = cx.theme().foreground;

        v_flex()
            .flex_1()
            .h_full()
            .bg(cx.theme().background)
            .overflow_hidden()
            .child(
                div()
                    .px_6()
                    .pt_6()
                    .pb_2()
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .text_color(cx.theme().foreground)
                    .child(i18n_settings(cx, "title")),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px_6()
                    .py_4()
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .w_full()
                            .gap_4()
                            .child(
                                self.render_card(
                                    i18n_settings(cx, "general"),
                                    border_color,
                                    fg,
                                    [
                                        self.render_row(
                                            cx,
                                            i18n_settings(cx, "launch_at_login"),
                                            i18n_settings(cx, "launch_at_login_desc"),
                                            Switch::new("launch-at-login")
                                                .checked(launch_at_login)
                                                .on_click(|enabled, _window, cx| {
                                                    if let Err(err) =
                                                        autostart::set_enabled(*enabled)
                                                    {
                                                        error!(error = %err, "failed to update autostart");
                                                        return;
                                                    }

                                                    let enabled = *enabled;
                                                    update_and_save(
                                                        cx,
                                                        "set_launch_at_login",
                                                        move |tide, _| {
                                                            tide.set_launch_at_login(enabled);
                                                        },
                                                    );
                                                })
                                                .small(),
                                        )
                                        .into_any_element(),
                                        self.render_row(
                                            cx,
                                            i18n_settings(cx, "show_main_window_on_startup"),
                                            i18n_settings(cx, "show_main_window_on_startup_desc"),
                                            Switch::new("show-main-window-on-startup")
                                                .checked(show_main_window_on_startup)
                                                .on_click(|enabled, _window, cx| {
                                                    let enabled = *enabled;
                                                    update_and_save(
                                                        cx,
                                                        "set_show_main_window_on_startup",
                                                        move |tide, _| {
                                                            tide.set_show_main_window_on_startup(
                                                                enabled,
                                                            );
                                                        },
                                                    );
                                                })
                                                .small(),
                                        )
                                        .into_any_element(),
                                        self.render_row(
                                            cx,
                                            i18n_settings(cx, "close_behavior"),
                                            i18n_settings(cx, "close_behavior_desc"),
                                            h_flex()
                                                .gap_2()
                                                .child(Self::option_button(
                                                    "close-hide-to-tray",
                                                    i18n_settings(cx, "close_hide_to_tray"),
                                                    close_behavior == CloseBehavior::HideToTray,
                                                    |_, _, cx| {
                                                        update_and_save(
                                                            cx,
                                                            "set_close_behavior",
                                                            |tide, _| {
                                                                tide.set_close_behavior(
                                                                    CloseBehavior::HideToTray,
                                                                );
                                                            },
                                                        );
                                                    },
                                                ))
                                                .child(Self::option_button(
                                                    "close-quit",
                                                    i18n_settings(cx, "close_quit"),
                                                    close_behavior == CloseBehavior::Quit,
                                                    |_, _, cx| {
                                                        update_and_save(
                                                            cx,
                                                            "set_close_behavior",
                                                            |tide, _| {
                                                                tide.set_close_behavior(
                                                                    CloseBehavior::Quit,
                                                                );
                                                            },
                                                        );
                                                    },
                                                )),
                                        )
                                        .into_any_element(),
                                        self.render_row(
                                            cx,
                                            i18n_settings(cx, "default_view"),
                                            i18n_settings(cx, "default_view_desc"),
                                            h_flex()
                                                .gap_2()
                                                .child(Self::option_button(
                                                    "default-view-last",
                                                    i18n_settings(cx, "default_view_last_opened"),
                                                    default_view == DefaultView::LastOpened,
                                                    |_, _, cx| {
                                                        update_and_save(
                                                            cx,
                                                            "set_default_view",
                                                            |tide, _| {
                                                                tide.set_default_view(
                                                                    DefaultView::LastOpened,
                                                                );
                                                            },
                                                        );
                                                    },
                                                ))
                                                .child(Self::option_button(
                                                    "default-view-all",
                                                    i18n_settings(cx, "default_view_all_tasks"),
                                                    default_view == DefaultView::AllTasks,
                                                    |_, _, cx| {
                                                        update_and_save(
                                                            cx,
                                                            "set_default_view",
                                                            |tide, _| {
                                                                tide.set_default_view(
                                                                    DefaultView::AllTasks,
                                                                );
                                                            },
                                                        );
                                                    },
                                                ))
                                                .child(Self::option_button(
                                                    "default-view-starred",
                                                    i18n_settings(cx, "default_view_starred"),
                                                    default_view == DefaultView::Starred,
                                                    |_, _, cx| {
                                                        update_and_save(
                                                            cx,
                                                            "set_default_view",
                                                            |tide, _| {
                                                                tide.set_default_view(
                                                                    DefaultView::Starred,
                                                                );
                                                            },
                                                        );
                                                    },
                                                ))
                                                .child(Self::option_button(
                                                    "default-view-first-group",
                                                    i18n_settings(cx, "default_view_first_group"),
                                                    default_view == DefaultView::FirstGroup,
                                                    |_, _, cx| {
                                                        update_and_save(
                                                            cx,
                                                            "set_default_view",
                                                            |tide, _| {
                                                                tide.set_default_view(
                                                                    DefaultView::FirstGroup,
                                                                );
                                                            },
                                                        );
                                                    },
                                                )),
                                        )
                                        .into_any_element(),
                                        self.render_row(
                                            cx,
                                            i18n_settings(cx, "completed_expanded_by_default"),
                                            i18n_settings(
                                                cx,
                                                "completed_expanded_by_default_desc",
                                            ),
                                            Switch::new("completed-expanded-by-default")
                                                .checked(completed_expanded_by_default)
                                                .on_click(|enabled, _window, cx| {
                                                    let enabled = *enabled;
                                                    update_and_save(
                                                        cx,
                                                        "set_completed_expanded_by_default",
                                                        move |tide, _| {
                                                            tide.set_completed_expanded_by_default(
                                                                enabled,
                                                            );
                                                        },
                                                    );
                                                })
                                                .small(),
                                        )
                                        .into_any_element(),
                                    ],
                                ),
                            )
                            .child(
                                self.render_card(
                                    i18n_settings(cx, "appearance"),
                                    border_color,
                                    fg,
                                    [
                                        self.render_row(
                                            cx,
                                            i18n_settings(cx, "language"),
                                            i18n_settings(cx, "language_desc"),
                                            h_flex()
                                                .gap_2()
                                                .child(Self::option_button(
                                                    "language-zh-cn",
                                                    "简体中文".to_string(),
                                                    locale == "zh-CN",
                                                    |_, _, cx| {
                                                        rust_i18n::set_locale("zh-CN");
                                                        gpui_component::set_locale("zh-CN");
                                                        tray::refresh_labels(cx, "zh-CN");
                                                        update_and_save(
                                                            cx,
                                                            "save_locale",
                                                            |tide, _| {
                                                                tide.set_locale(
                                                                    "zh-CN".to_string(),
                                                                );
                                                            },
                                                        );
                                                    },
                                                ))
                                                .child(Self::option_button(
                                                    "language-en",
                                                    "English".to_string(),
                                                    locale == "en",
                                                    |_, _, cx| {
                                                        rust_i18n::set_locale("en");
                                                        gpui_component::set_locale("en");
                                                        tray::refresh_labels(cx, "en");
                                                        update_and_save(
                                                            cx,
                                                            "save_locale",
                                                            |tide, _| {
                                                                tide.set_locale("en".to_string());
                                                            },
                                                        );
                                                    },
                                                )),
                                        )
                                        .into_any_element(),
                                        self.render_row(
                                            cx,
                                            i18n_settings(cx, "theme"),
                                            i18n_settings(cx, "theme_desc"),
                                            h_flex()
                                                .gap_2()
                                                .child(Self::option_button(
                                                    "theme-system",
                                                    i18n_settings(cx, "theme_system"),
                                                    theme.is_none(),
                                                    |_, _, cx| {
                                                        let render_mode =
                                                            match cx.window_appearance() {
                                                                WindowAppearance::Light => {
                                                                    ThemeMode::Light
                                                                }
                                                                _ => ThemeMode::Dark,
                                                            };
                                                        Theme::change(render_mode, None, cx);
                                                        update_and_save(
                                                            cx,
                                                            "save_theme",
                                                            |tide, _| {
                                                                tide.set_theme(None);
                                                            },
                                                        );
                                                    },
                                                ))
                                                .child(Self::option_button(
                                                    "theme-light",
                                                    i18n_settings(cx, "theme_light"),
                                                    theme == Some(ThemeMode::Light),
                                                    |_, _, cx| {
                                                        Theme::change(ThemeMode::Light, None, cx);
                                                        update_and_save(
                                                            cx,
                                                            "save_theme",
                                                            |tide, _| {
                                                                tide.set_theme(Some(
                                                                    ThemeMode::Light,
                                                                ));
                                                            },
                                                        );
                                                    },
                                                ))
                                                .child(Self::option_button(
                                                    "theme-dark",
                                                    i18n_settings(cx, "theme_dark"),
                                                    theme == Some(ThemeMode::Dark),
                                                    |_, _, cx| {
                                                        Theme::change(ThemeMode::Dark, None, cx);
                                                        update_and_save(
                                                            cx,
                                                            "save_theme",
                                                            |tide, _| {
                                                                tide.set_theme(Some(
                                                                    ThemeMode::Dark,
                                                                ));
                                                            },
                                                        );
                                                    },
                                                )),
                                        )
                                        .into_any_element(),
                                    ],
                                ),
                            )
                            .child(
                                self.render_card(
                                    i18n_settings(cx, "updates"),
                                    border_color,
                                    fg,
                                    [self
                                        .render_row(
                                            cx,
                                            i18n_settings(cx, "auto_check_updates"),
                                            i18n_settings(cx, "auto_check_updates_desc"),
                                            Switch::new("auto-check-updates")
                                                .checked(auto_check_updates)
                                                .on_click(|enabled, _window, cx| {
                                                    let enabled = *enabled;
                                                    update_and_save(
                                                        cx,
                                                        "set_auto_check_updates",
                                                        move |tide, _| {
                                                            tide.set_auto_check_updates(enabled);
                                                        },
                                                    );
                                                })
                                                .small(),
                                        )
                                        .into_any_element()],
                                ),
                            ),
                    ),
            )
    }
}
