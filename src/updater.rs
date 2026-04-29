//! Self-hosted update check and install.
use std::{
    cmp::Ordering,
    collections::HashMap,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering},
    },
    time::Duration,
};

use directories::ProjectDirs;
use gpui::{AnyWindowHandle, App, Context, IntoElement, Render, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme, WindowExt, dialog::DialogButtonProps, progress::Progress, v_flex,
};
use rust_i18n::t;
use serde::Deserialize;
use tracing::{error, warn};

use crate::{helpers::i18n_updater, state::TideStore};

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

pub fn check_and_show_available_only(handle: AnyWindowHandle, cx: &mut App) {
    let url = manifest_url().to_string();
    let current = CURRENT_VERSION.to_string();
    let platform = platform_key();

    cx.spawn(async move |cx| {
        let status = cx
            .background_spawn(async move { fetch_status(&url, &current, &platform) })
            .await;

        if matches!(status, UpdateStatus::Available { .. }) {
            let _ = handle.update(cx, |_any_view, window, cx| {
                show_status_dialog(window, cx, status);
            });
        }
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
                    // Close the "available" dialog ourselves so the framework's
                    // post-on_ok close doesn't pop the progress dialog that
                    // `start_update` is about to push.
                    window.close_dialog(cx);
                    start_update(window, cx, version.clone(), download_url.clone());
                    false
                }
            })
    });
}

/// View that renders a live progress bar driven by atomics shared with the
/// background download task.
struct ProgressView {
    desc: String,
    preparing: String,
    downloaded: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
}

impl Render for ProgressView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let downloaded = self.downloaded.load(AtomicOrdering::Relaxed);
        let total = self.total.load(AtomicOrdering::Relaxed);
        let percent = if total > 0 {
            (downloaded as f32 / total as f32 * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        let detail = if total > 0 {
            format!(
                "{} / {}  ({:.0}%)",
                format_bytes(downloaded),
                format_bytes(total),
                percent
            )
        } else if downloaded > 0 {
            format_bytes(downloaded)
        } else {
            self.preparing.clone()
        };

        v_flex()
            .gap_3()
            .child(div().text_sm().child(self.desc.clone()))
            .child(Progress::new().value(percent))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(detail),
            )
    }
}

fn format_bytes(b: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let v = b as f64;
    if v < KB {
        format!("{} B", b)
    } else if v < MB {
        format!("{:.1} KB", v / KB)
    } else if v < GB {
        format!("{:.1} MB", v / MB)
    } else {
        format!("{:.2} GB", v / GB)
    }
}

/// Kick off the background download and surface progress/result to the user.
fn start_update(window: &mut Window, cx: &mut App, version: String, url: String) {
    let handle = window.window_handle();
    let locale = cx.global::<TideStore>().read(cx).locale().to_string();
    let desc = t!(
        "updater.downloading_progress",
        version = version.as_str(),
        locale = locale.as_str()
    )
    .into_owned();
    let preparing = i18n_updater(cx, "preparing");
    let title = i18n_updater(cx, "downloading_title");

    let downloaded = Arc::new(AtomicU64::new(0));
    let total = Arc::new(AtomicU64::new(0));
    let done = Arc::new(AtomicBool::new(false));

    let progress_view = cx.new(|_| ProgressView {
        desc,
        preparing,
        downloaded: downloaded.clone(),
        total: total.clone(),
    });

    {
        let progress_view = progress_view.clone();
        window.open_dialog(cx, move |dialog, window, _cx| {
            let dialog_width = px(420.);
            let dialog_height = px(180.);
            let margin_top = ((window.viewport_size().height - dialog_height) / 2.).max(px(0.));
            dialog
                .title(title.clone())
                .child(progress_view.clone())
                .w(dialog_width)
                .margin_top(margin_top)
                .close_button(false)
                .overlay_closable(false)
                .keyboard(false)
        });
    }

    // Tick the view so the progress bar repaints while bytes flow in.
    let weak = progress_view.downgrade();
    let done_for_poll = done.clone();
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            if weak.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
            if done_for_poll.load(AtomicOrdering::Relaxed) {
                break;
            }
        }
    })
    .detach();

    cx.spawn(async move |cx| {
        let dl = downloaded.clone();
        let tt = total.clone();
        let result = cx
            .background_spawn(async move { download_with_progress(&url, dl, tt) })
            .await;
        done.store(true, AtomicOrdering::Relaxed);

        let _ = handle.update(cx, |_any, window, cx| {
            window.close_dialog(cx);
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

/// Best-effort: remove every file under the `updates/` cache directory.
/// Called both before a fresh download (to drop stale installers) and at app
/// startup (so the new version cleans up after itself once the user has
/// installed it).
pub fn clear_update_cache() {
    let dir = match update_cache_dir() {
        Ok(d) => d,
        Err(_) => return,
    };
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Err(e) = fs::remove_file(&path) {
            warn!(?path, error = %e, "failed to remove cached installer");
        }
    }
}

/// Stream the asset to `cache_dir/<filename>` using the last path segment of
/// the URL as the on-disk name. No timeout: updates can be large. Reports
/// progress via the shared atomics so the UI can render a live progress bar.
fn download_with_progress(
    url: &str,
    downloaded: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
) -> Result<PathBuf, String> {
    let cache_dir = update_cache_dir()?;
    // Drop any leftover installer from a previous (possibly aborted) download
    // so the cache stays a single-file directory.
    clear_update_cache();
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
    if let Some(len) = resp.body().content_length() {
        total.store(len, AtomicOrdering::Relaxed);
    }

    let mut file = fs::File::create(&dest).map_err(|e| format!("create {dest:?}: {e}"))?;
    let mut reader = resp.body_mut().as_reader();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("download: {e}")),
        };
        file.write_all(&buf[..n])
            .map_err(|e| format!("write: {e}"))?;
        downloaded.fetch_add(n as u64, AtomicOrdering::Relaxed);
    }

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
