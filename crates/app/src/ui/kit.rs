//! Small Linear-style building blocks shared by the views.

use crate::theme::{Palette, palette};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::progress::ProgressCircle;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use yadm_core::{Category, Download, State};

pub const ROW_H: f32 = 36.;

pub fn state_color(state: State, p: &Palette, cx: &App) -> Hsla {
    match state {
        State::Active => p.accent,
        State::Queued => cx.theme().muted_foreground,
        State::Paused => p.yellow,
        State::Failed => p.red,
        State::Completed => p.green,
        State::Removed => cx.theme().muted_foreground,
    }
}

pub fn state_icon(state: State) -> Lucide {
    match state {
        State::Active => Lucide::CircleArrowDown,
        State::Queued => Lucide::CircleDashed,
        State::Paused => Lucide::CirclePause,
        State::Failed => Lucide::CircleX,
        State::Completed => Lucide::CircleCheck,
        State::Removed => Lucide::CircleSlash,
    }
}

pub fn category_icon(c: Category) -> Lucide {
    match c {
        Category::Video => Lucide::Film,
        Category::Audio => Lucide::Music,
        Category::Documents => Lucide::FileText,
        Category::Archives => Lucide::FileArchive,
        Category::Software => Lucide::Package,
        Category::Images => Lucide::Image,
        Category::Torrents => Lucide::Magnet,
        Category::Other => Lucide::File,
    }
}

/// Linear's status glyph: a progress ring while downloading, an icon otherwise.
pub fn status_glyph(d: &Download, cx: &App) -> AnyElement {
    let p = palette(cx);
    let color = state_color(d.state, &p, cx);
    if d.state == State::Active {
        ProgressCircle::new(SharedString::from(format!("ring-{}", d.id)))
            .value(d.progress() * 100.)
            .color(color)
            .size_3p5()
            .into_any_element()
    } else {
        Icon::new(state_icon(d.state)).small().text_color(color).into_any_element()
    }
}

/// A 3px rounded progress track.
pub fn progress_bar(value: f32, color: Hsla, cx: &App) -> Div {
    div()
        .h(px(3.))
        .w_full()
        .rounded_full()
        .bg(cx.theme().muted)
        .child(div().h_full().rounded_full().bg(color).w(relative(value.clamp(0., 1.))))
}

pub fn muted_text(text: impl Into<SharedString>, cx: &App) -> Div {
    div().text_xs().text_color(cx.theme().muted_foreground).child(text.into())
}

/// A keyboard-shortcut hint, as in Linear's menus: one quiet pill, no per-key boxes.
pub fn kbd(keys: &str, cx: &App) -> Div {
    kbd_colored(keys, cx.theme().muted_foreground, cx.theme().muted)
}

/// Shortcut hint drawn on a colored surface (e.g. inside a primary button).
pub fn kbd_on(keys: &str, fg: Hsla) -> Div {
    kbd_colored(keys, fg.opacity(0.7), fg.opacity(0.14))
}

fn kbd_colored(keys: &str, fg: Hsla, bg: Hsla) -> Div {
    div()
        .px_1()
        .h(px(16.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(bg)
        .text_color(fg)
        .text_size(px(11.))
        .line_height(px(16.))
        .child(keys.replace(' ', ""))
}

/// Pill-shaped label (category chip).
pub fn chip(label: impl Into<SharedString>, icon: Option<Lucide>, cx: &App) -> Div {
    h_flex()
        .gap_1()
        .px_1p5()
        .h(px(20.))
        .rounded_full()
        .border_1()
        .border_color(cx.theme().border)
        .text_size(px(11.))
        .text_color(cx.theme().muted_foreground)
        .when_some(icon, |this, i| this.child(Icon::new(i).xsmall()))
        .child(label.into())
}

/// Section header used in the detail panel and settings.
pub fn section_label(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// A key/value row in the detail panel property list.
pub fn property(label: impl Into<SharedString>, value: impl IntoElement, cx: &App) -> Div {
    h_flex()
        .min_h(px(28.))
        .gap_3()
        .items_start()
        .child(
            div()
                .w(px(96.))
                .flex_shrink_0()
                .pt(px(5.))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label.into()),
        )
        .child(div().flex_1().min_w_0().pt(px(4.)).text_sm().child(value))
}
