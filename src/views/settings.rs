use gpui::{
    Anchor, AnyElement, App, AppContext, ClickEvent, Context, Entity, FontWeight, Hsla,
    InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled,
    Window, WindowAppearance, div, prelude::FluentBuilder, px, rgba,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Side, Sizable, Theme, ThemeMode,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    scroll::ScrollableElement,
    switch::Switch,
    tooltip::Tooltip,
    v_flex,
};
use tracing::error;

use crate::{
    ai::fetch_models,
    assets::CustomIconName,
    autostart,
    helpers::{active_item_bg, i18n_settings, interactive_accent},
    state::{
        AiProvider, AiSettings, CloseBehavior, DefaultView, OpenAiApiMode, TideStore,
        update_and_save, update_data_and_save,
    },
    tray,
};

enum AiValidationStatus {
    Success,
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsSection {
    General,
    Appearance,
    Ai,
    Notifications,
    Updates,
}

impl SettingsSection {
    const ALL: [Self; 5] = [
        Self::General,
        Self::Appearance,
        Self::Ai,
        Self::Notifications,
        Self::Updates,
    ];

    const fn id(self) -> &'static str {
        match self {
            Self::General => "settings-general",
            Self::Appearance => "settings-appearance",
            Self::Ai => "settings-ai",
            Self::Notifications => "settings-notifications",
            Self::Updates => "settings-updates",
        }
    }

    const fn label_key(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Appearance => "appearance",
            Self::Ai => "ai",
            Self::Notifications => "notifications",
            Self::Updates => "updates",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::General => IconName::Settings,
            Self::Appearance => IconName::Palette,
            Self::Ai => IconName::Bot,
            Self::Notifications => IconName::Bell,
            Self::Updates => IconName::Redo2,
        }
    }
}

pub struct SettingsView {
    section: SettingsSection,
    ai_provider: AiProvider,
    ai_api_key: Entity<InputState>,
    ai_endpoint: Entity<InputState>,
    ai_model: Entity<InputState>,
    ai_openai_api_mode: OpenAiApiMode,
    ai_thinking_enabled: bool,
    ai_validation: Option<(AiValidationStatus, String)>,
    ai_models: Vec<String>,
    ai_models_loading: bool,
    ai_models_status: Option<(AiValidationStatus, String)>,
}

impl SettingsView {
    fn ai_provider_icon(provider: AiProvider) -> Icon {
        let icon = match provider {
            AiProvider::OpenAi | AiProvider::OpenAiCompatible => CustomIconName::Openai,
            AiProvider::Claude => CustomIconName::Anthropic,
            AiProvider::Gemini => CustomIconName::Gemini,
            AiProvider::DeepSeek => CustomIconName::Deepseek,
            AiProvider::Ollama => CustomIconName::Ollama,
        };
        Icon::new(icon).size_4()
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ai = cx.global::<TideStore>().read(cx).ai().clone();
        let ai_api_key = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(ai.api_key.clone())
                .masked(true)
        });
        let ai_endpoint = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(ai.endpoint.clone())
                .placeholder("https://api.example.com/v1")
        });
        let ai_model = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(ai.model.clone())
                .placeholder("model-name")
        });

        Self {
            section: SettingsSection::General,
            ai_provider: ai.provider,
            ai_api_key,
            ai_endpoint,
            ai_model,
            ai_openai_api_mode: ai.openai_api_mode,
            ai_thinking_enabled: ai.thinking_enabled,
            ai_validation: None,
            ai_models: Vec::new(),
            ai_models_loading: false,
            ai_models_status: None,
        }
    }

    fn set_ai_provider(
        &mut self,
        provider: AiProvider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ai_provider = provider;
        self.ai_validation = None;
        self.ai_models.clear();
        self.ai_models_loading = false;
        self.ai_models_status = None;
        self.ai_api_key.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.ai_endpoint.update(cx, |input, cx| {
            input.set_value(provider.default_endpoint(), window, cx);
        });
        self.ai_model.update(cx, |input, cx| {
            input.set_value(provider.default_model(), window, cx);
        });
        cx.notify();
    }

    fn refresh_ai_models(&mut self, cx: &mut Context<Self>) {
        if self.ai_models_loading || !self.ai_provider.supports_model_listing() {
            return;
        }

        let settings = self.ai_settings(cx);
        if settings.provider.requires_api_key() && settings.api_key.is_empty() {
            self.ai_models.clear();
            self.ai_models_status = Some((
                AiValidationStatus::Error,
                i18n_settings(cx, "ai_models_require_key"),
            ));
            cx.notify();
            return;
        }
        if settings.endpoint.is_empty()
            || !(settings.endpoint.starts_with("http://")
                || settings.endpoint.starts_with("https://"))
        {
            self.ai_models_status = Some((
                AiValidationStatus::Error,
                i18n_settings(cx, "ai_endpoint_invalid"),
            ));
            cx.notify();
            return;
        }

        let provider = settings.provider;
        let request_endpoint = settings.endpoint.clone();
        let request_api_key = settings.api_key.clone();
        let weak = cx.entity().downgrade();
        self.ai_models_loading = true;
        self.ai_models_status = Some((
            AiValidationStatus::Success,
            i18n_settings(cx, "ai_models_loading"),
        ));
        cx.notify();

        cx.spawn(async move |_, cx| {
            let result = cx
                .background_spawn(async move { fetch_models(&settings) })
                .await;
            let _ = weak.update(cx, |this, cx| {
                let current_endpoint = this.ai_endpoint.read(cx).value().trim().to_string();
                let current_api_key = this.ai_api_key.read(cx).value().trim().to_string();
                if this.ai_provider != provider
                    || current_endpoint != request_endpoint
                    || current_api_key != request_api_key
                {
                    this.ai_models_loading = false;
                    cx.notify();
                    return;
                }

                this.ai_models_loading = false;
                match result {
                    Ok(models) => {
                        let count = models.len();
                        this.ai_models = models;
                        this.ai_models_status = Some((
                            AiValidationStatus::Success,
                            format!("{} {count}", i18n_settings(cx, "ai_models_loaded")),
                        ));
                    }
                    Err(err) => {
                        this.ai_models.clear();
                        this.ai_models_status = Some((
                            AiValidationStatus::Error,
                            format!("{} {err:#}", i18n_settings(cx, "ai_models_fetch_failed")),
                        ));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn ai_settings(&self, cx: &App) -> AiSettings {
        AiSettings {
            provider: self.ai_provider,
            api_key: self.ai_api_key.read(cx).value().trim().to_string(),
            endpoint: self.ai_endpoint.read(cx).value().trim().to_string(),
            model: self.ai_model.read(cx).value().trim().to_string(),
            openai_api_mode: self.ai_openai_api_mode,
            thinking_enabled: self.ai_thinking_enabled,
        }
    }

    fn validate_ai_settings(settings: &AiSettings, cx: &App) -> Result<String, String> {
        if settings.provider.requires_api_key() && settings.api_key.is_empty() {
            return Err(i18n_settings(cx, "ai_api_key_required"));
        }
        if settings.endpoint.is_empty() {
            return Err(i18n_settings(cx, "ai_endpoint_required"));
        }
        if !(settings.endpoint.starts_with("http://") || settings.endpoint.starts_with("https://"))
        {
            return Err(i18n_settings(cx, "ai_endpoint_invalid"));
        }
        if settings.model.is_empty() {
            return Err(i18n_settings(cx, "ai_model_required"));
        }

        Ok(i18n_settings(cx, "ai_validation_success"))
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
        border_color: Hsla,
        children: impl IntoIterator<Item = AnyElement>,
    ) -> AnyElement {
        v_flex()
            .max_w(px(760.))
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(border_color)
            .bg(rgba(0xffffff08))
            .px_4()
            .pb_1()
            .children(children)
            .into_any_element()
    }

    fn render_navigation_item(
        &self,
        section: SettingsSection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.section == section;
        let fg = if selected {
            interactive_accent(cx.theme())
        } else {
            cx.theme().foreground
        };

        h_flex()
            .id(section.id())
            .w_full()
            .gap_2()
            .px_3()
            .py_2()
            .rounded_lg()
            .cursor_pointer()
            .text_sm()
            .text_color(fg)
            .when(selected, |this| this.bg(active_item_bg(cx.theme())))
            .when(!selected, |this| {
                this.hover(|style| style.bg(cx.theme().secondary.opacity(0.55)))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.section = section;
                cx.notify();
            }))
            .child(Icon::new(section.icon()).size_4().flex_none())
            .child(i18n_settings(cx, section.label_key()))
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

    fn render_ai_row(
        &self,
        cx: &mut Context<Self>,
        title: String,
        control: impl IntoElement,
    ) -> AnyElement {
        h_flex()
            .w_full()
            .items_center()
            .gap_4()
            .py_2()
            .child(
                div()
                    .w(px(112.))
                    .flex_none()
                    .text_sm()
                    .font_weight(FontWeight(500.))
                    .text_color(cx.theme().foreground)
                    .child(title),
            )
            .child(div().flex_1().min_w_0().child(control))
            .into_any_element()
    }

    fn render_ai_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.entity().downgrade();
        let model_weak = weak.clone();
        let provider = self.ai_provider;
        let supports_model_listing = provider.supports_model_listing();
        let api_key_optional = !provider.requires_api_key();
        let border_color = cx.theme().border;
        let muted = cx.theme().muted_foreground;

        let provider_button = Button::new("ai-provider")
            .icon(Self::ai_provider_icon(provider))
            .label(provider.label())
            .dropdown_caret(true)
            .dropdown_menu(move |menu: PopupMenu, _, _| {
                AiProvider::ALL
                    .into_iter()
                    .fold(menu.check_side(Side::Right), |menu, candidate| {
                        let weak = weak.clone();
                        menu.item(
                            PopupMenuItem::new(candidate.label())
                                .icon(Self::ai_provider_icon(candidate))
                                .checked(candidate == provider)
                                .on_click(move |_, window, cx| {
                                    let _ = weak.update(cx, |this, cx| {
                                        this.set_ai_provider(candidate, window, cx);
                                    });
                                }),
                        )
                    })
            })
            .anchor(Anchor::TopLeft);

        let models = self.ai_models.clone();
        let no_models = models.is_empty();
        let no_models_label = i18n_settings(cx, "ai_no_models");
        let browse_models_button = Button::new("ai-browse-models")
            .label(i18n_settings(cx, "ai_browse_models"))
            .dropdown_caret(true)
            .disabled(self.ai_models_loading)
            .dropdown_menu(move |menu: PopupMenu, _, _| {
                if no_models {
                    return menu.item(PopupMenuItem::new(no_models_label.clone()).disabled(true));
                }

                models.iter().fold(menu, |menu, model| {
                    let weak = model_weak.clone();
                    let model = model.clone();
                    menu.item(
                        PopupMenuItem::new(model.clone()).on_click(move |_, window, cx| {
                            let _ = weak.update(cx, |this, cx| {
                                this.ai_model.update(cx, |input, cx| {
                                    input.set_value(&model, window, cx);
                                });
                                this.ai_validation = None;
                                cx.notify();
                            });
                        }),
                    )
                })
            })
            .anchor(Anchor::TopRight);

        let refresh_models_button = Button::new("ai-refresh-models")
            .icon(CustomIconName::Refresh)
            .loading(self.ai_models_loading)
            .disabled(!supports_model_listing)
            .on_click(cx.listener(|this, _, _, cx| {
                this.refresh_ai_models(cx);
            }));

        let status = self.ai_validation.as_ref().map(|(status, message)| {
            let color: Hsla = match status {
                AiValidationStatus::Success => rgba(0x16803cff).into(),
                AiValidationStatus::Error => cx.theme().danger,
            };
            div()
                .text_sm()
                .text_color(color)
                .child(message.clone())
                .into_any_element()
        });

        let test_button = Button::new("test-ai-settings")
            .label(i18n_settings(cx, "ai_test"))
            .on_click(cx.listener(|this, _, _, cx| {
                let settings = this.ai_settings(cx);
                this.ai_validation = Some(match Self::validate_ai_settings(&settings, cx) {
                    Ok(message) => (AiValidationStatus::Success, message),
                    Err(message) => (AiValidationStatus::Error, message),
                });
                cx.notify();
            }));

        let apply_button = Button::new("apply-ai-settings")
            .label(i18n_settings(cx, "ai_apply"))
            .primary()
            .on_click(cx.listener(|this, _, _, cx| {
                let settings = this.ai_settings(cx);
                match Self::validate_ai_settings(&settings, cx) {
                    Ok(message) => {
                        this.ai_validation = Some((AiValidationStatus::Success, message));
                        update_and_save(cx, "set_ai_settings", move |tide, _| {
                            tide.set_ai(settings.clone());
                        });
                    }
                    Err(message) => {
                        this.ai_validation = Some((AiValidationStatus::Error, message));
                    }
                }
                cx.notify();
            }));

        v_flex()
            .max_w(px(680.))
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(border_color)
            .bg(rgba(0xffffff08))
            .p_4()
            .gap_1()
            .child(
                v_flex().pb_3().child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(i18n_settings(cx, "ai_desc")),
                ),
            )
            .child(self.render_ai_row(cx, i18n_settings(cx, "ai_provider"), provider_button))
            .child(
                self.render_ai_row(
                    cx,
                    i18n_settings(cx, "ai_api_key"),
                    v_flex()
                        .gap_1()
                        .child(Input::new(&self.ai_api_key).mask_toggle().w_full().large())
                        .when(api_key_optional, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(i18n_settings(cx, "ai_api_key_optional")),
                            )
                        }),
                ),
            )
            .child(self.render_ai_row(
                cx,
                i18n_settings(cx, "ai_endpoint"),
                Input::new(&self.ai_endpoint).w_full().large(),
            ))
            .child(
                self.render_ai_row(
                    cx,
                    i18n_settings(cx, "ai_model"),
                    v_flex()
                        .gap_1()
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(Input::new(&self.ai_model).w_full().large()),
                                )
                                .when(supports_model_listing, |this| {
                                    this.child(browse_models_button)
                                })
                                .child(refresh_models_button),
                        )
                        .child(
                            self.ai_models_status
                                .as_ref()
                                .map(|(status, message)| {
                                    let color: Hsla = match status {
                                        AiValidationStatus::Success => muted,
                                        AiValidationStatus::Error => cx.theme().danger,
                                    };
                                    div()
                                        .text_xs()
                                        .text_color(color)
                                        .child(message.clone())
                                        .into_any_element()
                                })
                                .unwrap_or_else(|| {
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(i18n_settings(
                                            cx,
                                            if supports_model_listing {
                                                "ai_model_desc"
                                            } else {
                                                "ai_models_unsupported"
                                            },
                                        ))
                                        .into_any_element()
                                }),
                        ),
                ),
            )
            .when(provider.supports_openai_api_mode(), |this| {
                let chat_selected = self.ai_openai_api_mode == OpenAiApiMode::ChatCompletions;
                let responses_selected = self.ai_openai_api_mode == OpenAiApiMode::Responses;
                this.child(
                    self.render_ai_row(
                        cx,
                        i18n_settings(cx, "ai_api"),
                        h_flex()
                            .w_full()
                            .gap_2()
                            .child(Self::option_button(
                                "ai-api-chat-completions",
                                "/chat/completions".to_string(),
                                chat_selected,
                                cx.listener(|this, _, _, cx| {
                                    this.ai_openai_api_mode = OpenAiApiMode::ChatCompletions;
                                    this.ai_validation = None;
                                    cx.notify();
                                }),
                            ))
                            .child(Self::option_button(
                                "ai-api-responses",
                                "/responses".to_string(),
                                responses_selected,
                                cx.listener(|this, _, _, cx| {
                                    this.ai_openai_api_mode = OpenAiApiMode::Responses;
                                    this.ai_validation = None;
                                    cx.notify();
                                }),
                            )),
                    ),
                )
            })
            .child(
                self.render_ai_row(
                    cx,
                    i18n_settings(cx, "ai_thinking"),
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            Switch::new("ai-thinking")
                                .checked(self.ai_thinking_enabled)
                                .label(i18n_settings(cx, "ai_thinking_enabled"))
                                .on_click(cx.listener(|this, enabled, _, cx| {
                                    this.ai_thinking_enabled = *enabled;
                                    this.ai_validation = None;
                                    cx.notify();
                                }))
                                .small(),
                        )
                        .child(
                            div()
                                .id("ai-thinking-help")
                                .size(px(16.))
                                .flex_none()
                                .rounded_full()
                                .border_1()
                                .border_color(muted)
                                .text_xs()
                                .text_color(muted)
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child("?")
                                .tooltip(|window, cx| {
                                    Tooltip::element(|_, cx| {
                                        div()
                                            .w(px(360.))
                                            .whitespace_normal()
                                            .text_color(cx.theme().popover_foreground)
                                            .child(i18n_settings(cx, "ai_thinking_cost_desc"))
                                    })
                                    .build(window, cx)
                                }),
                        ),
                ),
            )
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .pt_4()
                    .mt_2()
                    .border_t_1()
                    .border_color(border_color)
                    .child(div().flex_1().min_w_0().children(status))
                    .child(h_flex().gap_2().child(test_button).child(apply_button)),
            )
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
        let notifications_enabled = config.notifications().enabled;
        let border_color = cx.theme().border;
        let fg = cx.theme().foreground;

        h_flex()
            .flex_1()
            .h_full()
            .bg(cx.theme().background)
            .overflow_hidden()
            .child(
                v_flex()
                    .w(px(188.))
                    .h_full()
                    .flex_none()
                    .border_r_1()
                    .border_color(border_color)
                    .bg(cx.theme().sidebar)
                    .px_3()
                    .py_5()
                    .gap_1()
                    .child(
                        h_flex()
                            .id("settings-back")
                            .w_full()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .mb_3()
                            .rounded_lg()
                            .cursor_pointer()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .hover(|style| style.bg(cx.theme().secondary.opacity(0.55)))
                            .on_click(|_, _, cx| {
                                update_data_and_save(cx, "return_from_settings", |data, _| {
                                    data.return_from_settings();
                                });
                            })
                            .child(Icon::new(IconName::ArrowLeft).size_4().flex_none())
                            .child(i18n_settings(cx, "back_to_app")),
                    )
                    .child(
                        div()
                            .px_3()
                            .pb_4()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(fg)
                            .child(i18n_settings(cx, "title")),
                    )
                    .children(
                        SettingsSection::ALL
                            .into_iter()
                            .map(|section| self.render_navigation_item(section, cx)),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scrollbar()
                    .child(
                        h_flex()
                            .w_full()
                            .justify_center()
                            .px_6()
                            .py_6()
                            .child(
                                v_flex()
                                    .w_full()
                                    .max_w(px(760.))
                                    .gap_5()
                                    .child(
                                        div()
                                            .text_2xl()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(fg)
                                            .child(i18n_settings(cx, self.section.label_key())),
                                    )
                            .when(self.section == SettingsSection::General, |this| {
                                            this.child(
                                                self.render_card(
                                                    border_color,
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
                            })
                            .when(self.section == SettingsSection::Appearance, |this| {
                                            this.child(
                                                self.render_card(
                                                    border_color,
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
                            })
                            .when(self.section == SettingsSection::Ai, |this| {
                                this.child(self.render_ai_card(cx))
                            })
                            .when(self.section == SettingsSection::Notifications, |this| {
                                            this.child(
                                                self.render_card(
                                                    border_color,
                                                    [self
                                        .render_row(
                                            cx,
                                            i18n_settings(cx, "task_notifications"),
                                            i18n_settings(cx, "task_notifications_desc"),
                                            Switch::new("task-notifications")
                                                .checked(notifications_enabled)
                                                .on_click(|enabled, _window, cx| {
                                                    let enabled = *enabled;
                                                    update_and_save(
                                                        cx,
                                                        "set_notifications_enabled",
                                                        move |tide, _| {
                                                            tide.set_notifications_enabled(enabled);
                                                        },
                                                    );
                                                })
                                                .small(),
                                        )
                                        .into_any_element()],
                                ),
                                )
                            })
                            .when(self.section == SettingsSection::Updates, |this| {
                                            this.child(
                                                self.render_card(
                                                    border_color,
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
                                )
                            })
                            ),
                    ),
            )
    }
}
