use std::borrow::Cow;

use anyhow::anyhow;
use gpui::{AssetSource, Result, SharedString};
use gpui_component::Icon;
use gpui_component_assets::Assets as ComponentAssets;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*.svg"]
#[include = "illustration/**/*.svg"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        if let Some(f) = ComponentAssets::get(path) {
            return Ok(Some(f.data));
        }

        Self::get(path)
            .map(|f| Some(f.data))
            .ok_or_else(|| anyhow!(r#"could not find asset at path "{path}""#))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut files: Vec<SharedString> = ComponentAssets::iter()
            .filter_map(|p| p.starts_with(path).then(|| p.into()))
            .collect();

        files.extend(
            Self::iter()
                .filter_map(|p| p.starts_with(path).then(|| p.into()))
                .collect::<Vec<_>>(),
        );

        Ok(files)
    }
}

pub enum CustomIconName {
    Logo,
    Star,
    StarOutline,
    Check,
    Trash,
    Clock,
    Refresh,
    Brain,
    // AI
    Anthropic,
    Deepseek,
    Gemini,
    Ollama,
    Openai,
}

impl CustomIconName {
    pub fn path(self) -> &'static str {
        match self {
            CustomIconName::Logo => "icons/tide.svg",
            CustomIconName::Star => "icons/star_filled.svg",
            CustomIconName::StarOutline => "icons/star_outline.svg",
            CustomIconName::Check => "icons/check.svg",
            CustomIconName::Trash => "icons/trash.svg",
            CustomIconName::Clock => "icons/clock.svg",
            CustomIconName::Refresh => "icons/refresh.svg",
            CustomIconName::Brain => "icons/brain.svg",
            // AI
            CustomIconName::Anthropic => "icons/ai/anthropic.svg",
            CustomIconName::Deepseek => "icons/ai/deepseek.svg",
            CustomIconName::Gemini => "icons/ai/googlegemini.svg",
            CustomIconName::Ollama => "icons/ai/ollama.svg",
            CustomIconName::Openai => "icons/ai/openai.svg",
        }
    }
}

impl From<CustomIconName> for Icon {
    fn from(val: CustomIconName) -> Self {
        Icon::empty().path(val.path())
    }
}
