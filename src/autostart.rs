#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use std::env;
#[cfg(target_os = "windows")]
use std::process::Command;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::{fs, path::PathBuf};

use anyhow::Result;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
use anyhow::bail;
#[cfg(target_os = "windows")]
use anyhow::{Context, bail};

#[cfg(target_os = "linux")]
use crate::PKG_NAME;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::helpers::get_home_directory;

#[cfg(target_os = "windows")]
const APP_NAME: &str = "Tide";
#[cfg(target_os = "macos")]
const APP_ID: &str = "com.yunisdu.tide";

pub fn set_enabled(enabled: bool) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        return set_enabled_macos(enabled);
    }

    #[cfg(target_os = "windows")]
    {
        return set_enabled_windows(enabled);
    }

    #[cfg(target_os = "linux")]
    {
        return set_enabled_linux(enabled);
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        bail!("autostart is not supported on this platform")
    }
}

#[cfg(target_os = "macos")]
fn set_enabled_macos(enabled: bool) -> Result<()> {
    let path = macos_launch_agent_path()?;

    if !enabled {
        if path.exists() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let exe = escape_xml(&env::current_exe()?.display().to_string());
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{APP_ID}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#
    );
    fs::write(path, plist)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_launch_agent_path() -> Result<PathBuf> {
    Ok(get_home_directory()?
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{APP_ID}.plist")))
}

#[cfg(target_os = "macos")]
fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "linux")]
fn set_enabled_linux(enabled: bool) -> Result<()> {
    let path = linux_autostart_path()?;

    if !enabled {
        if path.exists() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let exe = env::current_exe()?.display().to_string();
    let escaped_exe = exe.replace('\\', "\\\\").replace('"', "\\\"");
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=Tide\nExec=\"{escaped_exe}\"\nX-GNOME-Autostart-enabled=true\n"
    );
    fs::write(path, desktop)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_autostart_path() -> Result<PathBuf> {
    let config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or(get_home_directory()?.join(".config"));
    Ok(config_home
        .join("autostart")
        .join(format!("{PKG_NAME}.desktop")))
}

#[cfg(target_os = "windows")]
fn set_enabled_windows(enabled: bool) -> Result<()> {
    let exe = env::current_exe()?.display().to_string();
    let value = format!("\"{exe}\"");
    let status = if enabled {
        Command::new("reg")
            .args([
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                APP_NAME,
                "/t",
                "REG_SZ",
                "/d",
                &value,
                "/f",
            ])
            .status()
    } else {
        Command::new("reg")
            .args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                APP_NAME,
                "/f",
            ])
            .status()
    }
    .context("failed to run Windows registry command")?;

    if !status.success() {
        bail!("Windows registry command failed");
    }
    Ok(())
}
