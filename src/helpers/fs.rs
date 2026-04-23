use std::{fs, path::PathBuf};

use anyhow::{Result, anyhow, bail};
use directories::{ProjectDirs, UserDirs};

use crate::PKG_NAME;

pub fn get_or_create_config_path() -> Result<PathBuf> {
    let Some(project_dirs) = ProjectDirs::from("com", "yunisdu", PKG_NAME) else {
        bail!("project directories not found");
    };

    let config_dir = project_dirs.config_dir();

    if !config_dir.exists() {
        fs::create_dir_all(config_dir)?;
    }

    let config_path = config_dir.join("tide.toml");
    if config_path.exists() {
        return Ok(config_path);
    }
    std::fs::write(&config_path, "")?;

    Ok(config_path)
}

pub fn get_or_create_data_path() -> Result<PathBuf> {
    let home_dir = get_home_directory()?;
    let data_path = home_dir.join(".tide").join("data.json");

    if !data_path.exists() {
        std::fs::write(&data_path, "")?;
    }
    Ok(data_path)
}

pub fn get_home_directory() -> Result<PathBuf> {
    let user_dirs = UserDirs::new().ok_or(anyhow!("could not get user directory"))?;
    Ok(user_dirs.home_dir().to_path_buf())
}
