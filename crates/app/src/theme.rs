//! Linear-style theme for gpui-component, loaded from `themes/linear.json`.

use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{App, Hsla, WindowAppearance, rgb};
use yadm_core::ThemePref;

const LINEAR: &str = include_str!("../../../themes/linear.json");

pub fn init(pref: ThemePref, cx: &mut App) {
    if let Err(e) = ThemeRegistry::global_mut(cx).load_themes_from_str(LINEAR) {
        tracing::error!("failed to load Linear theme: {e:#}");
        return;
    }
    let themes = ThemeRegistry::global(cx).themes().clone();
    let light = themes.get("Linear Light").cloned();
    let dark = themes.get("Linear Dark").cloned();
    Theme::update(cx, |theme| {
        if let Some(l) = light {
            theme.light_theme = l;
        }
        if let Some(d) = dark {
            theme.dark_theme = d;
        }
    });
    apply(pref, cx);
}

pub fn apply(pref: ThemePref, cx: &mut App) {
    let mode = match pref {
        ThemePref::Light => ThemeMode::Light,
        ThemePref::Dark => ThemeMode::Dark,
        ThemePref::System => match cx.window_appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
            _ => ThemeMode::Light,
        },
    };
    Theme::change(mode, None, cx);
}

/// Status colors used for rings, chips and progress bars.
pub struct Palette {
    pub accent: Hsla,
    pub green: Hsla,
    pub yellow: Hsla,
    pub red: Hsla,
}

pub fn palette(cx: &App) -> Palette {
    let dark = Theme::global(cx).is_dark();
    let c = |d: u32, l: u32| -> Hsla { rgb(if dark { d } else { l }).into() };
    Palette {
        accent: c(0x5E6AD2, 0x5E6AD2),
        green: c(0x4CB782, 0x3BA16C),
        yellow: c(0xF2C94C, 0xD99A1E),
        red: c(0xEB5757, 0xE5484D),
    }
}
