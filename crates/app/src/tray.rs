//! macOS menu bar item showing live speed with quick controls.

use crate::actions::{NewDownload, PauseAll, Quit, ResumeAll};
use crate::store::AppStore;
use gpui_kit::*;
use std::time::Duration;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};
use yadm_core::format;

pub struct Tray {
    icon: TrayIcon,
    last_title: String,
}

/// A 18x18 template glyph: an arrow into a tray, drawn as RGBA.
fn glyph() -> tray_icon::Icon {
    const N: usize = 36; // @2x
    let mut px = vec![0u8; N * N * 4];
    let mut set = |x: usize, y: usize| {
        if x < N && y < N {
            let i = (y * N + x) * 4;
            px[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
        }
    };
    // Stem.
    for y in 4..22 {
        for x in 16..20 {
            set(x, y);
        }
    }
    // Arrow head.
    for i in 0..9 {
        for t in 0..4 {
            set(9 + i + t, 14 + i);
            set(26 - i - t, 14 + i);
        }
    }
    // Tray.
    for x in 5..31 {
        for y in 28..31 {
            set(x, y);
        }
    }
    for y in 22..31 {
        for x in 5..8 {
            set(x, y);
        }
        for x in 28..31 {
            set(x, y);
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
            .with_menu(Box::new(menu))
            .with_tooltip("YADM")
            .build()?;
        Ok((Self { icon, last_title: String::new() }, ids))
    }

    fn set_speed(&mut self, dl: u64, active: bool) {
        let title = if active && dl > 0 { format!(" {}", format::speed(dl)) } else { String::new() };
        if title != self.last_title {
            self.icon.set_title(if title.is_empty() { None } else { Some(title.clone()) });
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
                        Err(e) => tracing::warn!("menu bar item unavailable: {e:#}"),
                    },
                    (Some(_), false) => tray = None,
                    _ => {}
                }
                if let Some((t, ids)) = &mut tray {
                    t.set_speed(dl, active);
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
