//! Small Linear-style building blocks shared by the views.

use crate::theme::{Palette, palette};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::progress::ProgressCircle;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use yadm_core::{Category, Download, State};

pub const ROW_H: f32 = 40.;

/// Interface scale as f32 bits; see [`set_ui_scale`].
static UI_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000); // 1.0

pub fn ui_scale() -> f32 {
    f32::from_bits(UI_SCALE.load(std::sync::atomic::Ordering::Relaxed))
}

/// Sets the interface scale from the user's setting (`None` = system).
pub fn set_ui_scale(setting: Option<f32>) {
    let scale = setting.filter(|s| (0.5..=3.0).contains(s)).unwrap_or_else(crate::platform::system_ui_scale);
    UI_SCALE.store(scale.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// A size in design pixels, multiplied by the interface scale. Use this
/// instead of `px` for every layout size so the whole UI grows together.
pub fn dp(v: f32) -> Pixels {
    px(v * ui_scale())
}

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
        .h(dp(3.))
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
        .h(dp(18.))
        .flex()
        .items_center()
        .rounded(dp(4.))
        .bg(bg)
        .text_color(fg)
        .text_size(dp(12.))
        .line_height(dp(18.))
        .child(keys.replace(' ', ""))
}

/// Pill-shaped label (category chip).
pub fn chip(label: impl Into<SharedString>, icon: Option<Lucide>, cx: &App) -> Div {
    h_flex()
        .gap_1()
        .px_1p5()
        .h(dp(22.))
        .rounded_full()
        .border_1()
        .border_color(cx.theme().border)
        .text_size(dp(12.))
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
        .min_h(dp(30.))
        .gap_3()
        .items_start()
        .child(
            div()
                .w(dp(96.))
                .flex_shrink_0()
                .pt(dp(5.))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label.into()),
        )
        .child(div().flex_1().min_w_0().pt(dp(4.)).text_sm().child(value))
}

/// Empty strip at the top of a sidebar that clears the macOS traffic lights
/// and doubles as the window drag area. Elsewhere [`title_bar`] spans the
/// window instead, so the strip collapses.
pub fn window_drag_area(id: &'static str) -> Stateful<Div> {
    drag_region(div().id(id).h(dp(if crate::platform::IS_MAC { 40. } else { 0. })).flex_shrink_0())
}

/// Makes `el` move the window when dragged and maximize on double click.
fn drag_region(el: Stateful<Div>) -> Stateful<Div> {
    if cfg!(windows) {
        // Windows hit-tests this as the caption, which also gives native
        // double-click, snapping and the system menu.
        return el.window_control_area(WindowControlArea::Drag);
    }
    el.on_mouse_down(MouseButton::Left, |ev: &MouseDownEvent, window, _| {
        if ev.click_count >= 2 {
            window.titlebar_double_click();
        } else {
            window.start_window_move();
        }
    })
}

pub const TITLE_BAR_H: f32 = 34.;

/// Window caption for Windows and client-decorated Linux: a drag strip with
/// minimize / maximize / close on the right. `None` where the OS draws its
/// own controls (macOS traffic lights, server-side decorations).
pub fn title_bar(window: &Window, cx: &App) -> Option<AnyElement> {
    if crate::platform::IS_MAC {
        return None;
    }
    if cfg!(target_os = "linux") && !matches!(window.window_decorations(), Decorations::Client { .. }) {
        return None;
    }
    let supported = window.window_controls();
    let control = |id: &'static str, icon: Lucide, area: WindowControlArea, close: bool| {
        let theme = cx.theme();
        let (hover_bg, hover_fg) = if close { (theme.danger, theme.danger_foreground) } else { (theme.secondary_hover, theme.foreground) };
        div()
            .id(id)
            .w(dp(46.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .text_color(theme.muted_foreground)
            .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
            .when(cfg!(windows), |el| el.window_control_area(area))
            .when(!cfg!(windows), |el| {
                el.on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    match area {
                        WindowControlArea::Min => window.minimize_window(),
                        WindowControlArea::Max => window.zoom_window(),
                        _ => {
                            if crate::should_close_window(window, cx) {
                                window.remove_window();
                            }
                        }
                    }
                })
            })
            .child(Icon::new(icon).small())
    };
    Some(
        h_flex()
            .h(dp(TITLE_BAR_H))
            .w_full()
            .flex_shrink_0()
            .child(drag_region(div().id("title-bar-drag").flex_1().h_full()))
            .when(supported.minimize, |el| el.child(control("win-min", Lucide::Minus, WindowControlArea::Min, false)))
            .when(supported.maximize, |el| {
                let icon = if window.is_maximized() { Lucide::Copy } else { Lucide::Square };
                el.child(control("win-max", icon, WindowControlArea::Max, false))
            })
            .child(control("win-close", Lucide::X, WindowControlArea::Close, true))
            .into_any_element(),
    )
}

/// Snappy, slightly underdamped spring for press feedback.
const PRESS_SPRING: SpringConfig = SpringConfig::new(700., 34., 1.);
/// How far a pressed control dims.
const PRESS_OPACITY: f32 = 0.6;

/// Wraps a clickable element so it dims while held and springs back on
/// release, giving every button a small tactile response.
#[derive(IntoElement)]
pub struct Pressable {
    id: ElementId,
    child: AnyElement,
}

pub trait PressableExt: IntoElement + Sized {
    /// `id` must be unique among siblings (reusing the element's own id is fine).
    fn pressable(self, id: impl Into<ElementId>) -> Pressable {
        Pressable { id: id.into(), child: self.into_any_element() }
    }
}

impl<E: IntoElement> PressableExt for E {}

impl RenderOnce for Pressable {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let pressed = window.use_keyed_state(self.id.clone(), cx, |_, _| false);
        let is_pressed = *pressed.read(cx);
        let setter = |value: bool| {
            let pressed = pressed.clone();
            move |cx: &mut App| {
                pressed.update(cx, |p, cx| {
                    if *p != value {
                        *p = value;
                        cx.notify();
                    }
                })
            }
        };
        let (press, release, release_out) = (setter(true), setter(false), setter(false));
        div()
            .id(self.id.clone())
            .on_mouse_down(MouseButton::Left, move |_, _, cx| press(cx))
            .on_mouse_up(MouseButton::Left, move |_, _, cx| release(cx))
            .on_mouse_up_out(MouseButton::Left, move |_, _, cx| release_out(cx))
            .child(self.child)
            .with_spring(self.id, SpringAnimation::new(PRESS_SPRING).to(is_pressed), |el, phase| {
                el.opacity(phase.interpolate(1., PRESS_OPACITY).clamp(PRESS_OPACITY, 1.))
            })
    }
}
