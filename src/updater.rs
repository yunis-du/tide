//! Self-hosted update check and install.
//!
//! Flow:
//! 1. Fetch a JSON manifest from a configurable URL.
//! 2. Compare `version` against the running `CARGO_PKG_VERSION`.
//! 3. If newer: show an "Update Available" dialog.
//! 4. On user confirmation: stream-download the platform-specific asset to the
//!    user's cache directory while showing a persistent notification.
//! 5. When the download finishes: show an "Update Ready" dialog.
//! 6. On user confirmation: spawn the platform installer and quit the app so
//!    the installer can replace the running binary/bundle.
//!
//! The manifest URL defaults to a placeholder and can be overridden at compile
//! time via the `TIDE_UPDATE_URL` env var, e.g.
//! `TIDE_UPDATE_URL=https://example.com/tide/latest.json cargo build --release`.
//!
//! Expected manifest shape:
//! ```json
//! {
//!   "version": "0.2.0",
//!   "notes": "Release notes shown in the dialog.",
//!   "pub_date": "2026-04-25T00:00:00Z",
//!   "platforms": {
//!     "macos-aarch64": { "url": "https://.../Tide_0.2.0_aarch64.dmg" },
//!     "macos-x86_64":  { "url": "https://.../Tide_0.2.0_x64.dmg" },
//!     "windows-x86_64": { "url": "https://.../Tide_0.2.0_x64-setup.exe" },
//!     "linux-x86_64":   { "url": "https://.../Tide_0.2.0_amd64.AppImage" }
//!   }
//! }
//! ```
//! `pub_date` and `notes` are optional; `platforms` keys are `{os}-{arch}`.

use std::{
    cmp::Ordering,
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use directories::ProjectDirs;
use gpui::{App, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme, WindowExt, dialog::DialogButtonProps, notification::Notification,
};
use rust_i18n::t;
use serde::Deserialize;
use tracing::{error, warn};

use crate::{helpers::i18n_updater, state::TideStore};

/// Marker type so the "downloading" notification can be dismissed by id once
/// the background download completes.
struct UpdatingNotification;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const REQUEST_TIMEOUT_SECS: u64 = 10;
const FALLBACK_MANIFEST_URL: &str = "http://download.yunisdu.com/tide/latest.json";

fn manifest_url() -> &'static str {
    option_env!("TIDE_UPDATE_URL").unwrap_or(FALLBACK_MANIFEST_URL)
}

fn platform_key() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "linux" => "linux",
        other => other,
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

#[derive(Debug, Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    platforms: HashMap<String, PlatformEntry>,
}

#[derive(Debug, Deserialize)]
struct PlatformEntry {
    url: String,
}

#[derive(Debug, Clone)]
pub enum UpdateStatus {
    UpToDate {
        current: String,
    },
    Available {
        version: String,
        notes: Option<String>,
        download_url: String,
    },
    Failed {
        error: String,
    },
}

/// Compare dotted version strings (e.g. `0.2.0` vs `0.10.1`).
/// Numeric segments are compared numerically; non-numeric segments fall back
/// to lexicographic comparison so pre-release tags don't cause a panic.
fn compare_versions(a: &str, b: &str) -> Ordering {
    let mut ai = a.split('.');
    let mut bi = b.split('.');
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(xn), Ok(yn)) => xn.cmp(&yn),
                    _ => x.cmp(y),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

fn fetch_status(url: &str, current: &str, platform: &str) -> UpdateStatus {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(REQUEST_TIMEOUT_SECS)))
        .build()
        .into();

    let manifest: Manifest = match agent.get(url).call() {
        Ok(mut resp) => match resp.body_mut().read_json() {
            Ok(m) => m,
            Err(e) => {
                return UpdateStatus::Failed {
                    error: format!("parse manifest: {e}"),
                };
            }
        },
        Err(e) => {
            return UpdateStatus::Failed {
                error: format!("fetch manifest: {e}"),
            };
        }
    };

    if compare_versions(current, &manifest.version) != Ordering::Less {
        return UpdateStatus::UpToDate {
            current: current.to_string(),
        };
    }

    // Newer version exists but no asset for this platform: treat as up-to-date
    // so the user isn't pointed at a non-existent download.
    let Some(entry) = manifest.platforms.get(platform) else {
        warn!(platform, version = %manifest.version, "manifest missing platform entry");
        return UpdateStatus::UpToDate {
            current: current.to_string(),
        };
    };

    UpdateStatus::Available {
        version: manifest.version,
        notes: manifest.notes,
        download_url: entry.url.clone(),
    }
}

/// Entry point called from the title-bar menu item. Kicks off the background
/// check and shows a dialog on the main window with the result.
pub fn check_and_show(window: &mut Window, cx: &mut App) {
    let handle = window.window_handle();
    let url = manifest_url().to_string();
    let current = CURRENT_VERSION.to_string();
    let platform = platform_key();

    cx.spawn(async move |cx| {
        let status = cx
            .background_spawn(async move { fetch_status(&url, &current, &platform) })
            .await;

        // Use AnyWindowHandle::update here (not `downcast::<Root>`): the
        // latter borrows `Root` mutably and `Window::open_dialog` re-enters
        // the same entity, which would panic with "already being updated".
        let _ = handle.update(cx, |_any_view, window, cx| {
            show_status_dialog(window, cx, status);
        });
    })
    .detach();
}

fn show_status_dialog(window: &mut Window, cx: &mut App, status: UpdateStatus) {
    let locale = cx.global::<TideStore>().read(cx).locale().to_string();
    match status {
        UpdateStatus::Available {
            version,
            notes,
            download_url,
        } => open_available_dialog(window, cx, &locale, version, notes, download_url),
        UpdateStatus::UpToDate { current } => {
            let desc = t!(
                "updater.up_to_date_desc",
                version = current.as_str(),
                locale = locale.as_str()
            )
            .into_owned();
            open_info_dialog(window, cx, i18n_updater(cx, "up_to_date_title"), desc);
        }
        UpdateStatus::Failed { error } => {
            let desc = t!(
                "updater.failed_desc",
                error = error.as_str(),
                locale = locale.as_str()
            )
            .into_owned();
            open_info_dialog(window, cx, i18n_updater(cx, "failed_title"), desc);
        }
    }
}

fn open_available_dialog(
    window: &mut Window,
    cx: &mut App,
    locale: &str,
    version: String,
    notes: Option<String>,
    download_url: String,
) {
    let title = i18n_updater(cx, "available_title");
    let desc = t!(
        "updater.available_desc",
        version = version.as_str(),
        locale = locale
    )
    .into_owned();
    let download_label = i18n_updater(cx, "download_btn");
    let later_label = i18n_updater(cx, "later_btn");
    let notes = notes.filter(|n| !n.trim().is_empty());

    window.open_dialog(cx, move |dialog, window, cx| {
        let dialog_width = px(420.);
        let dialog_height = px(220.);
        let margin_top = ((window.viewport_size().height - dialog_height) / 2.).max(px(0.));
        let muted = cx.theme().muted_foreground;
        let download_url = download_url.clone();

        let body = match notes.clone() {
            Some(notes_text) => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_sm().child(desc.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .whitespace_normal()
                        .child(notes_text),
                )
                .into_any_element(),
            None => div().text_sm().child(desc.clone()).into_any_element(),
        };

        dialog
            .title(title.clone())
            .child(body)
            .w(dialog_width)
            .margin_top(margin_top)
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text(download_label.clone())
                    .cancel_text(later_label.clone()),
            )
            .on_ok({
                let download_url = download_url.clone();
                let version = version.clone();
                move |_, window, cx| {
                    start_update(window, cx, version.clone(), download_url.clone());
                    true
                }
            })
    });
}

/// Kick off the background download and surface progress/result to the user.
fn start_update(window: &mut Window, cx: &mut App, version: String, url: String) {
    let handle = window.window_handle();
    let locale = cx.global::<TideStore>().read(cx).locale().to_string();
    let downloading_msg = t!(
        "updater.downloading_msg",
        version = version.as_str(),
        locale = locale.as_str()
    )
    .into_owned();

    window.push_notification(
        Notification::info(downloading_msg)
            .id::<UpdatingNotification>()
            .autohide(false),
        cx,
    );

    cx.spawn(async move |cx| {
        let result = cx
            .background_spawn(async move { download_update_sync(&url) })
            .await;

        let _ = handle.update(cx, |_any, window, cx| {
            window.remove_notification::<UpdatingNotification>(cx);
            match result {
                Ok(path) => open_ready_dialog(window, cx, version, path),
                Err(e) => open_download_failed_dialog(window, cx, e),
            }
        });
    })
    .detach();
}

fn update_cache_dir() -> Result<PathBuf, String> {
    let project_dirs = ProjectDirs::from("com", "yunisdu", crate::PKG_NAME)
        .ok_or_else(|| "project dirs not found".to_string())?;
    let dir = project_dirs.cache_dir().join("updates");
    fs::create_dir_all(&dir).map_err(|e| format!("create cache dir: {e}"))?;
    Ok(dir)
}

/// Stream the asset to `cache_dir/<filename>` using the last path segment of
/// the URL as the on-disk name. No timeout: updates can be large.
fn download_update_sync(url: &str) -> Result<PathBuf, String> {
    let cache_dir = update_cache_dir()?;
    let file_name = url
        .rsplit('/')
        .next()
        .and_then(|s| s.split('?').next())
        .filter(|s| !s.is_empty())
        .unwrap_or("tide-update.bin")
        .to_string();
    let dest = cache_dir.join(&file_name);

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(None)
        .build()
        .into();

    let mut resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    let mut file = fs::File::create(&dest).map_err(|e| format!("create {dest:?}: {e}"))?;
    io::copy(&mut resp.body_mut().as_reader(), &mut file)
        .map_err(|e| format!("download: {e}"))?;

    Ok(dest)
}

/// Spawn the platform installer and return once it has successfully detached.
/// Caller is responsible for quitting the app afterwards.
fn launch_installer(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // Delegates to `open`, which mounts .dmg / launches .pkg and lets the
        // user drag the new .app into /Applications. Sparkle-style in-place
        // replacement would require a .app.tar.gz asset instead of a .dmg.
        Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("open installer: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "msi" {
            Command::new("msiexec")
                .arg("/i")
                .arg(path)
                .spawn()
                .map_err(|e| format!("start msiexec: {e}"))?;
        } else {
            Command::new(path)
                .spawn()
                .map_err(|e| format!("start installer: {e}"))?;
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = fs::metadata(path).map_err(|e| format!("stat {path:?}: {e}"))?;
        let mut perms = meta.permissions();
        perms.set_mode(perms.mode() | 0o755);
        fs::set_permissions(path, perms).map_err(|e| format!("chmod {path:?}: {e}"))?;
        Command::new(path)
            .spawn()
            .map_err(|e| format!("exec update: {e}"))?;
    }
    Ok(())
}

fn open_ready_dialog(window: &mut Window, cx: &mut App, version: String, installer: PathBuf) {
    let locale = cx.global::<TideStore>().read(cx).locale().to_string();
    let title = i18n_updater(cx, "ready_title");
    let desc = t!(
        "updater.ready_desc",
        version = version.as_str(),
        locale = locale.as_str()
    )
    .into_owned();
    let install_label = i18n_updater(cx, "install_btn");
    let later_label = i18n_updater(cx, "later_btn");

    window.open_dialog(cx, move |dialog, window, _cx| {
        let dialog_width = px(420.);
        let dialog_height = px(180.);
        let margin_top = ((window.viewport_size().height - dialog_height) / 2.).max(px(0.));
        let installer_path = installer.clone();

        dialog
            .title(title.clone())
            .child(div().text_sm().child(desc.clone()))
            .w(dialog_width)
            .margin_top(margin_top)
            .confirm()
            .button_props(
                DialogButtonProps::default()
                    .ok_text(install_label.clone())
                    .cancel_text(later_label.clone()),
            )
            .on_ok(move |_, _, cx| {
                match launch_installer(&installer_path) {
                    Ok(()) => cx.quit(),
                    Err(e) => error!(error = %e, "failed to launch installer"),
                }
                true
            })
    });
}

fn open_download_failed_dialog(window: &mut Window, cx: &mut App, error: String) {
    let locale = cx.global::<TideStore>().read(cx).locale().to_string();
    let desc = t!(
        "updater.download_failed_desc",
        error = error.as_str(),
        locale = locale.as_str()
    )
    .into_owned();
    open_info_dialog(window, cx, i18n_updater(cx, "download_failed_title"), desc);
}

fn open_info_dialog(window: &mut Window, cx: &mut App, title: String, desc: String) {
    let ok_label = i18n_updater(cx, "ok_btn");
    window.open_dialog(cx, move |dialog, window, _cx| {
        let dialog_width = px(360.);
        let dialog_height = px(160.);
        let margin_top = ((window.viewport_size().height - dialog_height) / 2.).max(px(0.));
        dialog
            .title(title.clone())
            .child(div().text_sm().child(desc.clone()))
            .w(dialog_width)
            .margin_top(margin_top)
            .alert()
            .button_props(DialogButtonProps::default().ok_text(ok_label.clone()))
            .on_ok(|_, _, _| true)
    });
}
