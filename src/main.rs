#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

#[cfg(not(target_os = "linux"))]
use gpui::TitlebarOptions;
use gpui::{
    App, AppContext, Bounds, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, Styled, Window, WindowAppearance, WindowBounds, WindowHandle, WindowOptions,
    prelude::FluentBuilder, px, size,
};
use gpui_component::{
    ActiveTheme, Root, Sizable, Theme, ThemeMode, button::Button, h_flex, v_flex,
};
use rust_i18n::t;

use crate::{
    helpers::{HotKeyAction, LocaleAction, ThemeAction, default_ui_locale, new_hot_keys},
    state::{
        CloseBehavior, DefaultView, SidebarSelection, TideDataStore, TideStore,
        data::load_data,
        persistence::{Persistence, wait_for_save},
        tide::{load_config, save_config},
        update_and_save,
    },
    views::{ContentView, SidebarView, TitleBarView},
};

use crate::views::floating::floating_window_ids;

rust_i18n::i18n!("locales", fallback = "en");

const PKG_NAME: &str = env!("CARGO_PKG_NAME");

mod ai;
mod assets;
mod autostart;
mod components;
mod helpers;
mod notifications;
#[cfg(target_os = "windows")]
mod single_instance;
mod state;
mod tray;
mod updater;
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

/// Force the platform window to stay above all other windows. GPUI's
/// `WindowKind::PopUp` already does this on macOS (via `NSPopUpWindowLevel`)
/// but on Windows it only sets `WS_EX_TOOLWINDOW`, which keeps it out of the
/// taskbar but doesn't make it topmost.
pub(crate) fn set_window_always_on_top(_window: &Window) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
        };

        if let Some(hwnd) = window_hwnd(_window) {
            unsafe {
                if let Err(e) = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                ) {
                    tracing::warn!(error = %e, "failed to make floating window topmost");
                }
            }
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
    let floating: std::collections::HashSet<_> = floating_window_ids().into_iter().collect();

    #[cfg(target_os = "windows")]
    {
        for handle in cx.windows() {
            if floating.contains(&handle.window_id()) {
                continue;
            }
            let _ = handle.update(cx, |_, window, _| hide_on_windows(window));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if floating.is_empty() {
            cx.hide();
        } else {
            // Some groups are pinned; minimize the main window only so the
            // floating notes stay on screen.
            for handle in cx.windows() {
                if floating.contains(&handle.window_id()) {
                    continue;
                }
                let _ = handle.update(cx, |_, window, _| window.minimize_window());
            }
        }
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
        let settings_open = cx.global::<TideDataStore>().read(cx).sidebar_selection()
            == &SidebarSelection::Settings;
        let persistence = cx.global::<Persistence>();
        let storage_notice = if persistence.has_errors() {
            Some((t!("persistence.save_failed").to_string(), true))
        } else if persistence.recovered {
            Some((t!("persistence.recovered").to_string(), false))
        } else {
            None
        };
        let storage_banner = storage_notice.map(|(message, retry)| {
            h_flex()
                .px_4()
                .py_2()
                .gap_3()
                .bg(cx.theme().muted)
                .child(gpui::div().flex_1().text_sm().child(message))
                .child(
                    Button::new("storage-action")
                        .label(
                            if retry {
                                t!("persistence.retry")
                            } else {
                                t!("persistence.dismiss")
                            }
                            .to_string(),
                        )
                        .small()
                        .on_click(move |_, _, cx| {
                            if retry {
                                Persistence::retry(cx);
                            } else {
                                cx.global_mut::<Persistence>().recovered = false;
                                cx.refresh_windows();
                            }
                        }),
                )
        });

        v_flex()
            .id(PKG_NAME)
            .bg(cx.theme().background)
            .size_full()
            .child(self.titlebar.clone())
            .children(storage_banner)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .when(!settings_open, |this| this.child(self.sidebar.clone()))
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
                if _cx.global::<TideStore>().read(_cx).close_behavior() == CloseBehavior::Quit {
                    _cx.quit();
                    return false;
                }

                #[cfg(target_os = "windows")]
                {
                    hide_on_windows(_window);
                }
                #[cfg(not(target_os = "windows"))]
                {
                    if !floating_window_ids().is_empty() {
                        _window.remove_window();
                    } else {
                        _cx.hide();
                    }
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

    let app = gpui_platform::application().with_assets(assets::Assets);

    rust_i18n::set_locale(default_ui_locale());
    let loaded_config = match load_config() {
        Ok(loaded) => loaded,
        Err(error) => {
            show_storage_error(&error);
            return;
        }
    };
    let mut app_config = loaded_config.value;
    if app_config.locale.is_none() {
        app_config.locale = Some(default_ui_locale().to_string());
        if let Err(error) = save_config(&app_config) {
            show_storage_error(&error);
            return;
        }
    }

    // Apply the locale before any data load: first-run creates a default
    // task group whose name is resolved via `t!`, so the locale must be
    // set or the name falls back to English.
    let locale = app_config.locale.as_deref().unwrap_or("en");
    rust_i18n::set_locale(locale);

    let loaded_data = match load_data() {
        Ok(loaded) => loaded,
        Err(error) => {
            show_storage_error(&error);
            return;
        }
    };
    let persistence = match Persistence::new(
        loaded_config.recovered || loaded_data.recovered,
        app_config.ai().clone(),
    ) {
        Ok(persistence) => persistence,
        Err(error) => {
            show_storage_error(&error);
            return;
        }
    };
    let mut task_data = loaded_data.value;
    if task_data.task_groups.is_empty() {
        task_data
            .task_groups
            .push(crate::state::data::TaskGroup::default_group());
    }
    match app_config.default_view() {
        DefaultView::LastOpened => {}
        DefaultView::AllTasks => task_data.sidebar_selection = SidebarSelection::AllTasks,
        DefaultView::Starred => task_data.sidebar_selection = SidebarSelection::Starred,
        DefaultView::FirstGroup => {
            task_data.sidebar_selection = task_data
                .task_groups
                .last()
                .map(|group| SidebarSelection::Group(group.id.clone()))
                .unwrap_or(SidebarSelection::AllTasks);
        }
    }

    app.run(move |cx| {
        gpui_component::init(cx);
        gpui_component::set_locale(app_config.locale.as_deref().unwrap_or("en"));
        cx.set_global(persistence);
        cx.on_app_quit(|cx| {
            // GPUI only gives async shutdown hooks 100 ms. Drain the independent
            // writer here before returning a completed shutdown future.
            if let Err(error) =
                futures::executor::block_on(wait_for_save(cx.global::<Persistence>().queue.flush()))
            {
                rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Error)
                    .set_title(t!("persistence.error_title").as_ref())
                    .set_description(t!("persistence.quit_failed", error = error).as_ref())
                    .show();
            }
            async {}
        })
        .detach();

        // Drop the previous run's cached installer (if any) on startup. We
        // can't safely delete it right after spawning the installer process
        // (Windows holds the .exe lock; macOS may still be reading the .dmg),
        // so the cleanup happens here once the new version is running.
        cx.background_executor()
            .spawn(async { updater::clear_update_cache() })
            .detach();

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
        notifications::spawn(cx);

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
            let handle = cx.update(|cx| open_main_window(cx))?;

            // Tray must be created on the main thread after the event loop is
            // running (macOS requirement); doing it from this spawned task
            // satisfies that ordering.
            cx.update(|cx| tray::init(cx, handle));

            let show_main_window = cx.update(|cx| {
                cx.global::<TideStore>()
                    .read(cx)
                    .show_main_window_on_startup()
            });
            if !show_main_window {
                cx.update(|cx| hide_to_tray(cx));
            }

            let auto_check_updates =
                cx.update(|cx| cx.global::<TideStore>().read(cx).auto_check_updates());
            if auto_check_updates {
                let any_handle = handle.into();
                cx.update(|cx| updater::check_and_show_available_only(any_handle, cx));
            }

            #[cfg(target_os = "windows")]
            if let Some(guard) = instance_guard {
                cx.update(|cx| single_instance::spawn_watcher(cx, guard, handle));
            }

            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}

fn show_storage_error(error: &anyhow::Error) {
    tracing::error!(error = %error, "Cannot load local storage");
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(t!("persistence.error_title").as_ref())
        .set_description(t!("persistence.load_failed", error = format!("{error:#}")).as_ref())
        .show();
}
