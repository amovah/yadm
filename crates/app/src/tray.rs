//! Menu bar item (macOS) / system tray icon (Windows) showing live
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

fn glyph() -> tray_icon::Icon {
    use crate::tray_glyph::{N, rgba};
    tray_icon::Icon::from_rgba(rgba(!cfg!(target_os = "macos")), N as u32, N as u32).expect("tray icon")
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
