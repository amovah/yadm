//! Menu bar item (macOS) / system tray icon (Windows, Linux) showing live
//! speed with quick controls.

use crate::actions::{NewDownload, PauseAll, Quit, ResumeAll};
use crate::store::AppStore;
use gpui_kit::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use yadm_core::format;

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether the tray icon is currently shown, i.e. a closed window can be
/// brought back through it.
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub struct Tray {
    icon: TrayIcon,
    last_title: String,
}

/// Template glyph matching the app icon: a 3/4 progress ring around a down arrow.
/// Drawn at 36x36 (@2x of 18pt) with anti-aliased edges via distance fields.
/// macOS tints the black template itself; elsewhere it's drawn in the app's
/// indigo, which reads on both light and dark taskbars.
fn glyph() -> tray_icon::Icon {
    const N: usize = 36;
    let c = (N as f32 - 1.) / 2.;
    let (ring_r, ring_w, stroke) = (14.5f32, 3.2f32, 3.2f32);
    // Distance from p to segment ab.
    let seg = |px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32| {
        let (dx, dy) = (bx - ax, by - ay);
        let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0., 1.);
        ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt()
    };
    let mut px = vec![0u8; N * N * 4];
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32, y as f32);
            let (dx, dy) = (fx - c, fy - c);
            // Angle clockwise from 12 o'clock, 0..1.
            let ang = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.);
            let ring = (dx.hypot(dy) - ring_r).abs() - ring_w / 2.;
            // Round caps at the arc ends (0 and 0.75 turns).
            let cap = |a: f32| {
                let a = a * std::f32::consts::TAU;
                (fx - (c + ring_r * a.sin())).hypot(fy - (c - ring_r * a.cos())) - ring_w / 2.
            };
            let arc = if ang <= 0.75 { ring } else { cap(0.).min(cap(0.75)) };
            let arrow = seg(fx, fy, c, c - 7.5, c, c + 7.).min(seg(fx, fy, c - 6., c + 1., c, c + 7.)).min(seg(fx, fy, c + 6., c + 1., c, c + 7.))
                - stroke / 2.;
            let d = arc.min(arrow);
            let a = (0.5 - d).clamp(0., 1.);
            let p = &mut px[(y * N + x) * 4..][..4];
            if !cfg!(target_os = "macos") {
                p[..3].copy_from_slice(&[0x5E, 0x6A, 0xD2]);
            }
            p[3] = (a * 255.) as u8;
        }
    }
    tray_icon::Icon::from_rgba(px, N as u32, N as u32).expect("tray icon")
}

impl Tray {
    pub fn new() -> anyhow::Result<(Self, Vec<(tray_icon::menu::MenuId, &'static str)>)> {
        let show = MenuItem::new("Show YADM", true, None);
        let add = MenuItem::new("New Download…", true, None);
        let pause = MenuItem::new("Pause All", true, None);
        let resume = MenuItem::new("Resume All", true, None);
        let quit = MenuItem::new("Quit YADM", true, None);
        let menu = Menu::new();
        menu.append_items(&[&show, &add, &PredefinedMenuItem::separator(), &pause, &resume, &PredefinedMenuItem::separator(), &quit])?;
        let ids = vec![
            (show.id().clone(), "show"),
            (add.id().clone(), "add"),
            (pause.id().clone(), "pause"),
            (resume.id().clone(), "resume"),
            (quit.id().clone(), "quit"),
        ];
        let icon = TrayIconBuilder::new()
            .with_icon(glyph())
            .with_icon_as_template(true)
            // Windows convention: left click opens the app, right click the menu.
            .with_menu_on_left_click(cfg!(target_os = "macos"))
            .with_menu(Box::new(menu))
            .with_tooltip("YADM")
            .build()?;
        Ok((Self { icon, last_title: String::new() }, ids))
    }

    fn set_speed(&mut self, dl: u64, active: bool) {
        let title = if active && dl > 0 { format!(" {}", format::speed(dl)) } else { String::new() };
        if title != self.last_title {
            // Titles show next to the icon on macOS only; the tooltip works on Windows.
            self.icon.set_title(if title.is_empty() { None } else { Some(title.clone()) });
            let tip = if title.is_empty() { "YADM".to_string() } else { format!("YADM —{title}") };
            if let Err(e) = self.icon.set_tooltip(Some(tip)) {
                tracing::debug!("tray tooltip: {e}");
            }
            self.last_title = title;
        }
    }
}

fn show_main_window(cx: &mut App) {
    cx.activate(true);
    if let Some(w) = cx.windows().first() {
        w.update(cx, |_, window, _| window.activate_window()).ok();
    } else {
        crate::open_main_window(cx);
    }
}

/// Creates the tray (if enabled) and keeps it in sync with the store.
pub fn start(cx: &mut App) {
    let store = AppStore::global(cx);
    cx.spawn(async move |cx: &mut AsyncApp| {
        let mut tray: Option<(Tray, Vec<(tray_icon::menu::MenuId, &'static str)>)> = None;
        loop {
            cx.background_executor().timer(Duration::from_millis(400)).await;
            let keep = cx.update(|cx| {
                let s = store.read(cx);
                let want = s.settings().show_tray;
                let (dl, active) = (s.snapshot().global.dl_speed, s.snapshot().global.active > 0);
                match (&mut tray, want) {
                    (None, true) => match Tray::new() {
                        Ok(t) => tray = Some(t),
                        Err(e) => tracing::warn!("tray icon unavailable: {e:#}"),
                    },
                    (Some(_), false) => tray = None,
                    _ => {}
                }
                ACTIVE.store(tray.is_some(), Ordering::Relaxed);
                if let Some((t, ids)) = &mut tray {
                    t.set_speed(dl, active);
                    while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
                        if !cfg!(target_os = "macos")
                            && let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev
                        {
                            show_main_window(cx);
                        }
                    }
                    while let Ok(ev) = MenuEvent::receiver().try_recv() {
                        let Some((_, what)) = ids.iter().find(|(id, _)| *id == ev.id) else { continue };
                        match *what {
                            "show" => show_main_window(cx),
                            "add" => {
                                show_main_window(cx);
                                if let Some(w) = cx.windows().first() {
                                    w.update(cx, |_, window, cx| window.dispatch_action(Box::new(NewDownload), cx)).ok();
                                }
                            }
                            "pause" => cx.dispatch_action(&PauseAll),
                            "resume" => cx.dispatch_action(&ResumeAll),
                            "quit" => cx.dispatch_action(&Quit),
                            _ => {}
                        }
                    }
                }
                true
            });
            if !keep {
                break;
            }
        }
    })
    .detach();
}
