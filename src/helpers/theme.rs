use gpui::Hsla;
use gpui_component::Theme;

pub fn interactive_accent(theme: &Theme) -> Hsla {
    if theme.is_dark() {
        theme.blue
    } else {
        theme.info_active
    }
}

pub fn active_item_bg(theme: &Theme) -> Hsla {
    let opacity = if theme.is_dark() { 0.2 } else { 0.15 };
    interactive_accent(theme).opacity(opacity)
}
