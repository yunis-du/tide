//! System tray integration.
//!
//! On macOS / Windows this module creates a tray icon with a menu (Show / Quit),
//! binds clicks on the icon to activating the main window
//!
//! Linux is intentionally skipped (would require libappindicator + a gtk loop
//! that GPUI does not run); the public API degrades to no-ops.

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use platform::{init, refresh_labels, update_badge};

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn init(_cx: &mut gpui::App, _window: gpui::WindowHandle<gpui_component::Root>) {}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn update_badge(_cx: &gpui::App) {}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn refresh_labels(_cx: &gpui::App, _locale: &str) {}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod platform {
    use std::cell::RefCell;
    use std::io::Cursor;
    use std::time::Duration;

    use chrono::Local;
    use gpui::{App, AsyncApp, WindowHandle};
    use gpui_component::Root;
    use rust_i18n::t;
    use tracing::{error, warn};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
        menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    };

    use crate::state::TideDataStore;
    use crate::{
        helpers::{i18n_tray, i18n_tray_tooltip_count},
        open_main_window,
        views::open_about_window,
    };

    struct TrayHandle {
        tray: TrayIcon,
        show_item: MenuItem,
        about_item: MenuItem,
        quit_item: MenuItem,
    }

    // The TrayIcon must outlive the app but is `!Send` (holds `Rc<RefCell<_>>`).
    // A thread-local keeps it alive on the main thread where we create it.
    thread_local! {
        static TRAY: RefCell<Option<TrayHandle>> = const { RefCell::new(None) };
    }

    const SHOW_ID: &str = "tide.tray.show";
    const ABOUT_ID: &str = "tide.tray.about";
    const QUIT_ID: &str = "tide.tray.quit";

    const ICON_BYTES: &[u8] = include_bytes!("../assets/logos/icons/32x32.png");

    struct BaseIcon {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    }

    fn decode_base_icon() -> anyhow::Result<BaseIcon> {
        let decoder = png::Decoder::new(Cursor::new(ICON_BYTES));
        let mut reader = decoder.read_info()?;
        let output_buffer_size = reader
            .output_buffer_size()
            .ok_or_else(|| anyhow::anyhow!("png decoder did not report output buffer size"))?;
        let mut buf = vec![0u8; output_buffer_size];
        let info = reader.next_frame(&mut buf)?;
        if info.color_type != png::ColorType::Rgba {
            anyhow::bail!("expected RGBA png, got {:?}", info.color_type);
        }
        let rgba = buf[..info.buffer_size()].to_vec();
        Ok(BaseIcon {
            rgba,
            width: info.width,
            height: info.height,
        })
    }

    fn load_icon() -> anyhow::Result<Icon> {
        let base = decode_base_icon()?;
        Ok(Icon::from_rgba(base.rgba, base.width, base.height)?)
    }

    #[cfg(target_os = "windows")]
    fn load_icon_with_count(count: usize) -> anyhow::Result<Icon> {
        let mut base = decode_base_icon()?;
        if count > 0 {
            draw_count_badge(&mut base.rgba, base.width, base.height, count);
        }
        Ok(Icon::from_rgba(base.rgba, base.width, base.height)?)
    }

    /// Paint a red circular badge with the count in the top-right corner.
    /// Uses a 3x5 bitmap font scaled to the badge size.
    #[cfg(target_os = "windows")]
    fn draw_count_badge(rgba: &mut [u8], width: u32, height: u32, count: usize) {
        let badge_size = (width.min(height) * 5 / 8).max(12) as i32;
        let badge_x = width as i32 - badge_size;
        let badge_y = 0_i32;
        let radius = badge_size as f32 / 2.0;
        let cx = badge_x as f32 + radius;
        let cy = badge_y as f32 + radius;

        for y in badge_y..badge_y + badge_size {
            for x in badge_x..badge_x + badge_size {
                if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                    continue;
                }
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let dist = (dx * dx + dy * dy).sqrt();
                let alpha = if dist <= radius - 1.0 {
                    1.0
                } else if dist < radius {
                    radius - dist
                } else {
                    continue;
                };
                let idx = ((y as u32 * width + x as u32) * 4) as usize;
                rgba[idx] = 0xE5;
                rgba[idx + 1] = 0x3B;
                rgba[idx + 2] = 0x30;
                rgba[idx + 3] = (alpha * 255.0) as u8;
            }
        }

        let text = if count > 9 {
            "9+".to_string()
        } else {
            count.to_string()
        };
        let char_count = text.chars().count() as i32;
        let scale = (badge_size / 8).max(2);
        let char_w = 3 * scale;
        let char_h = 5 * scale;
        let spacing = scale;
        let total_w = char_w * char_count + spacing * (char_count - 1).max(0);
        let start_x = badge_x + (badge_size - total_w) / 2;
        let start_y = badge_y + (badge_size - char_h) / 2;

        for (i, c) in text.chars().enumerate() {
            let Some(glyph) = glyph_bitmap(c) else {
                continue;
            };
            let char_x = start_x + (char_w + spacing) * i as i32;
            for gy in 0..5 {
                for gx in 0..3 {
                    if glyph[gy * 3 + gx] == 0 {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let px = char_x + gx as i32 * scale + dx;
                            let py = start_y + gy as i32 * scale + dy;
                            if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                                continue;
                            }
                            let idx = ((py as u32 * width + px as u32) * 4) as usize;
                            rgba[idx] = 0xFF;
                            rgba[idx + 1] = 0xFF;
                            rgba[idx + 2] = 0xFF;
                            rgba[idx + 3] = 0xFF;
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn glyph_bitmap(c: char) -> Option<[u8; 15]> {
        #[rustfmt::skip]
        let pixels = match c {
            '0' => [1,1,1, 1,0,1, 1,0,1, 1,0,1, 1,1,1],
            '1' => [0,1,0, 1,1,0, 0,1,0, 0,1,0, 1,1,1],
            '2' => [1,1,0, 0,0,1, 0,1,0, 1,0,0, 1,1,1],
            '3' => [1,1,0, 0,0,1, 0,1,0, 0,0,1, 1,1,0],
            '4' => [1,0,1, 1,0,1, 1,1,1, 0,0,1, 0,0,1],
            '5' => [1,1,1, 1,0,0, 1,1,0, 0,0,1, 1,1,0],
            '6' => [0,1,1, 1,0,0, 1,1,0, 1,0,1, 0,1,0],
            '7' => [1,1,1, 0,0,1, 0,1,0, 0,1,0, 0,1,0],
            '8' => [0,1,0, 1,0,1, 0,1,0, 1,0,1, 0,1,0],
            '9' => [0,1,0, 1,0,1, 0,1,1, 0,0,1, 1,1,0],
            '+' => [0,0,0, 0,1,0, 1,1,1, 0,1,0, 0,0,0],
            _ => return None,
        };
        Some(pixels)
    }

    pub fn init(cx: &mut App, window: WindowHandle<Root>) {
        let menu = Menu::new();
        let show_item = MenuItem::with_id(SHOW_ID, i18n_tray(cx, "show"), true, None);
        let about_item = MenuItem::with_id(ABOUT_ID, i18n_tray(cx, "about"), true, None);
        let quit_item = MenuItem::with_id(QUIT_ID, i18n_tray(cx, "quit"), true, None);
        if let Err(e) = menu.append_items(&[
            &show_item,
            &about_item,
            &PredefinedMenuItem::separator(),
            &quit_item,
        ]) {
            error!(error = %e, "failed to build tray menu");
            return;
        }

        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip(i18n_tray(cx, "tooltip"));
        match load_icon() {
            Ok(icon) => builder = builder.with_icon(icon),
            Err(e) => warn!(error = %e, "tray icon missing, using text-only tray"),
        }

        let tray = match builder.build() {
            Ok(t) => t,
            Err(e) => {
                error!(error = %e, "failed to build tray icon");
                return;
            }
        };
        TRAY.with(|cell| {
            *cell.borrow_mut() = Some(TrayHandle {
                tray,
                show_item,
                about_item,
                quit_item,
            })
        });

        spawn_event_loop(cx, window);
        update_badge(cx);
    }

    fn spawn_event_loop(cx: &App, window: WindowHandle<Root>) {
        cx.spawn(async move |cx: &mut AsyncApp| {
            let mut main_window = window;
            let menu_rx = MenuEvent::receiver();
            let tray_rx = TrayIconEvent::receiver();
            loop {
                while let Ok(event) = menu_rx.try_recv() {
                    match event.id.0.as_str() {
                        SHOW_ID => ensure_main_window(cx, &mut main_window).await,
                        ABOUT_ID => {
                            let _ = cx.update(|cx| open_about_window(cx));
                        }
                        QUIT_ID => {
                            let _ = cx.update(|cx| cx.quit());
                        }
                        _ => {}
                    }
                }
                while let Ok(event) = tray_rx.try_recv() {
                    if let TrayIconEvent::Click {
                        button,
                        button_state,
                        ..
                    } = event
                        && button == MouseButton::Left
                        && button_state == MouseButtonState::Up
                    {
                        ensure_main_window(cx, &mut main_window).await;
                    }
                }
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
            }
        })
        .detach();
    }

    async fn ensure_main_window(cx: &mut AsyncApp, main_window: &mut WindowHandle<Root>) {
        if activate_window(cx, *main_window).await {
            return;
        }

        match cx.update(|cx| open_main_window(cx)) {
            Ok(Ok(handle)) => {
                *main_window = handle;
                let _ = activate_window(cx, *main_window).await;
            }
            Ok(Err(e)) => error!(error = %e, "failed to reopen main window"),
            Err(e) => error!(error = %e, "failed to update app for reopen"),
        }
    }

    async fn activate_window(cx: &mut AsyncApp, window: WindowHandle<Root>) -> bool {
        let _ = cx.update(|cx| cx.activate(true));
        window
            .update(cx, |_, w, _| {
                #[cfg(target_os = "windows")]
                crate::show_on_windows(w);
                w.activate_window();
            })
            .is_ok()
    }

    fn today_pending_count(cx: &App) -> usize {
        let today = Local::now().date_naive();
        let data = cx.global::<TideDataStore>().read(cx);
        data.tasks
            .iter()
            .filter(|t| t.parent_id.is_none() && !t.is_completed && t.due_date == Some(today))
            .count()
    }

    pub fn update_badge(cx: &App) {
        let count = today_pending_count(cx);
        let tooltip = if count > 0 {
            i18n_tray_tooltip_count(cx, count)
        } else {
            i18n_tray(cx, "tooltip")
        };
        TRAY.with(|cell| {
            if let Some(handle) = cell.borrow().as_ref() {
                apply_count(handle, count);
                if let Err(e) = handle.tray.set_tooltip(Some(tooltip.as_str())) {
                    warn!(error = %e, "failed to update tray tooltip");
                }
            }
        });
    }

    #[cfg(target_os = "windows")]
    fn apply_count(handle: &TrayHandle, count: usize) {
        match load_icon_with_count(count) {
            Ok(icon) => {
                if let Err(e) = handle.tray.set_icon(Some(icon)) {
                    warn!(error = %e, "failed to update tray icon");
                }
            }
            Err(e) => warn!(error = %e, "failed to render tray icon"),
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn apply_count(_handle: &TrayHandle, _count: usize) {}

    /// Re-apply tray menu labels for the given locale. Takes the locale
    /// explicitly so it doesn't race the (async) `TideStore` update that
    /// persists the new language setting.
    pub fn refresh_labels(_cx: &App, locale: &str) {
        let show: String = t!("tray.show", locale = locale).into();
        let about: String = t!("tray.about", locale = locale).into();
        let quit: String = t!("tray.quit", locale = locale).into();
        TRAY.with(|cell| {
            if let Some(handle) = cell.borrow().as_ref() {
                handle.show_item.set_text(&show);
                handle.about_item.set_text(&about);
                handle.quit_item.set_text(&quit);
            }
        });
        #[cfg(target_os = "windows")]
        {
            // Tooltip depends on count + locale; refresh via update_badge would
            // read locale from the store, which may still hold the old value at
            // this point, so compute the tooltip here with the new locale.
            let count = today_pending_count(_cx);
            let tooltip: String = if count > 0 {
                t!(
                    "tray.tooltip_count",
                    count = count.to_string(),
                    locale = locale
                )
                .into()
            } else {
                t!("tray.tooltip", locale = locale).into()
            };
            TRAY.with(|cell| {
                if let Some(handle) = cell.borrow().as_ref() {
                    let _ = handle.tray.set_tooltip(Some(&tooltip));
                }
            });
        }
    }
}
