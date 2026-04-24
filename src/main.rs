#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

#[cfg(not(target_os = "linux"))]
use gpui::TitlebarOptions;
use gpui::{
    App, AppContext, Application, Bounds, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, Styled, Window, WindowAppearance, WindowBounds, WindowHandle,
    WindowOptions, px, size,
};
use gpui_component::{ActiveTheme, Root, Theme, ThemeMode, h_flex, v_flex};

use crate::{
    helpers::{HotKeyAction, LocaleAction, ThemeAction, default_ui_locale, new_hot_keys},
    state::{
        TideDataStore, TideStore,
        data::load_data,
        tide::{load_config, save_config},
        update_and_save,
    },
    views::{ContentView, SidebarView, TitleBarView},
};

rust_i18n::i18n!("locales", fallback = "en");

const PKG_NAME: &str = env!("CARGO_PKG_NAME");

mod assets;
mod components;
mod helpers;
#[cfg(target_os = "windows")]
mod single_instance;
mod state;
mod tray;
mod views;

#[cfg(target_os = "windows")]
fn window_hwnd(window: &Window) -> Option<windows::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::c_void;
    use windows::Win32::Foundation::HWND;

    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        return None;
    };
    Some(HWND(raw.hwnd.get() as *mut c_void))
}

#[cfg(target_os = "windows")]
fn hide_on_windows(window: &Window) {
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};

    // GPUI's App::hide() is a no-op on Windows, so we hide the native HWND directly.
    if let Some(hwnd) = window_hwnd(window) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn show_on_windows(window: &Window) {
    use windows::Win32::UI::WindowsAndMessaging::{
        IsIconic, SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow,
    };

    if let Some(hwnd) = window_hwnd(window) {
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            } else {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

fn hide_to_tray(cx: &mut App) {
    #[cfg(target_os = "windows")]
    {
        for handle in cx.windows() {
            let _ = handle.update(cx, |_, window, _| hide_on_windows(window));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        cx.hide();
    }
}

pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

struct Tide {
    titlebar: Entity<TitleBarView>,
    sidebar: Entity<SidebarView>,
    content: Entity<ContentView>,
}

impl Tide {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let titlebar = cx.new(|cx| TitleBarView::new(window, cx));
        let sidebar = cx.new(|cx| SidebarView::new(window, cx));
        let content = cx.new(|cx| ContentView::new(window, cx));

        cx.observe_window_appearance(window, |_this, _window, cx| {
            if cx.global::<TideStore>().read(cx).theme().is_none() {
                Theme::change(cx.window_appearance(), None, cx);
                cx.refresh_windows();
            }
        })
        .detach();

        Self {
            titlebar,
            sidebar,
            content,
        }
    }
}

impl Render for Tide {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog_layer = Root::render_dialog_layer(_window, cx);

        v_flex()
            .id(PKG_NAME)
            .bg(cx.theme().background)
            .size_full()
            .child(self.titlebar.clone())
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar.clone())
                    .child(self.content.clone()),
            )
            .children(dialog_layer)
    }
}

pub(crate) fn open_main_window(cx: &mut App) -> anyhow::Result<WindowHandle<Root>> {
    let window_bounds = {
        let window_size = size(px(960.), px(640.));
        Bounds::centered(None, window_size, cx)
    };

    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(window_bounds)),
            #[cfg(not(target_os = "linux"))]
            titlebar: Some(TitlebarOptions {
                title: None,
                appears_transparent: true,
                traffic_light_position: Some(gpui::point(px(9.0), px(9.0))),
            }),
            show: true,
            is_resizable: true,
            ..Default::default()
        },
        |window, cx| {
            // Keep About and other windows independent when main window closes.
            window.on_window_should_close(cx, move |_window, _cx| {
                #[cfg(target_os = "windows")]
                {
                    hide_on_windows(_window);
                }
                #[cfg(not(target_os = "windows"))]
                {
                    _cx.hide();
                }
                false
            });
            let content_view = cx.new(|cx| Tide::new(window, cx));
            cx.new(|cx| Root::new(content_view, window, cx))
        },
    )
}

fn main() {
    #[cfg(target_os = "windows")]
    let instance_guard = match single_instance::acquire() {
        Ok(single_instance::Acquired::First(guard)) => Some(guard),
        Ok(single_instance::Acquired::AlreadyRunning) => return,
        Err(e) => {
            tracing::warn!(error = %e, "single-instance check failed; continuing");
            None
        }
    };

    let app = Application::new().with_assets(assets::Assets);

    // Load persisted config; fall back to defaults on error.
    let app_config = load_config().unwrap_or_default();
    let mut task_data = load_data().unwrap_or_default();

    let mut app_config = app_config;
    if app_config.locale.is_none() {
        app_config.locale = Some(default_ui_locale().to_string());
        let _ = save_config(&app_config);
    }

    if task_data.task_groups.is_empty() {
        task_data
            .task_groups
            .push(crate::state::data::TaskGroup::default_group());
    }

    app.run(move |cx| {
        gpui_component::init(cx);
        gpui_component::set_locale(app_config.locale.as_deref().unwrap_or("en"));

        cx.activate(true);

        // Register the two independent globals.
        let config_entity = cx.new(|_| app_config.clone());
        let tasks_entity = cx.new(|_| task_data.clone());

        if let Some(theme) = TideStore::new(config_entity.clone()).read(cx).theme() {
            Theme::change(theme, None, cx);
        }

        cx.set_global(TideStore::new(config_entity));
        cx.set_global(TideDataStore::new(tasks_entity.clone()));
        cx.bind_keys(new_hot_keys());

        // Refresh the tray badge whenever task data changes.
        cx.observe(&tasks_entity, |_, cx| tray::update_badge(cx))
            .detach();

        cx.on_action(|action: &ThemeAction, cx: &mut App| {
            let mode = match action {
                ThemeAction::Light => Some(ThemeMode::Light),
                ThemeAction::Dark => Some(ThemeMode::Dark),
                ThemeAction::System => None,
            };

            let render_mode = match mode {
                Some(m) => m,
                None => match cx.window_appearance() {
                    WindowAppearance::Light => ThemeMode::Light,
                    _ => ThemeMode::Dark,
                },
            };

            Theme::change(render_mode, None, cx);

            update_and_save(cx, "save_theme", move |tide, _| {
                tide.set_theme(mode);
            });
        });

        cx.on_action(|action: &LocaleAction, cx: &mut App| {
            let locale = match action {
                LocaleAction::ZhCN => "zh-CN",
                LocaleAction::En => "en",
            };
            rust_i18n::set_locale(locale);
            gpui_component::set_locale(locale);
            tray::refresh_labels(cx, locale);

            update_and_save(cx, "save_locale", move |tide, _| {
                tide.set_locale(locale.to_string());
            });
        });

        cx.on_action(|e: &HotKeyAction, cx: &mut App| match e {
            HotKeyAction::Quit => cx.quit(),
            HotKeyAction::Hide => {
                hide_to_tray(cx);
            }
        });

        cx.spawn(async move |cx| {
            let handle = cx.update(|cx| open_main_window(cx))??;

            // Tray must be created on the main thread after the event loop is
            // running (macOS requirement); doing it from this spawned task
            // satisfies that ordering.
            cx.update(|cx| tray::init(cx, handle))?;

            #[cfg(target_os = "windows")]
            if let Some(guard) = instance_guard {
                cx.update(|cx| single_instance::spawn_watcher(cx, guard, handle))?;
            }

            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}
