//! System tray icon on Linux, over the StatusNotifierItem D-Bus protocol.
//!
//! KDE, Xfce, Cinnamon and others show these natively; GNOME needs the
//! AppIndicator extension (enabled by default on Ubuntu). No GTK involved: ksni
//! runs the D-Bus service on its own thread and hands clicks back through a
//! channel that the gpui side drains.

use crate::actions::{NewDownload, PauseAll, Quit, ResumeAll};
use crate::store::AppStore;
use crate::tray_glyph::{N, rgba};
use gpui_kit::*;
use ksni::blocking::{Handle, TrayMethods as _};
use ksni::menu::StandardItem;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;
use yadm_core::format;

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether the tray icon is currently shown, i.e. a closed window can be
/// brought back through it.
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

#[derive(Clone, Copy)]
enum Click {
    Show,
    Add,
    Pause,
    Resume,
    Quit,
}

struct YadmTray {
    tx: Sender<Click>,
    /// Live speed shown in the tooltip, empty when idle.
    speed: String,
    icon: Vec<ksni::Icon>,
}

impl ksni::Tray for YadmTray {
    fn id(&self) -> String {
        "dev.yadm.app".into()
    }

    fn title(&self) -> String {
        "YADM".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: if self.speed.is_empty() { "YADM".into() } else { format!("YADM — {}", self.speed) },
            ..Default::default()
        }
    }

    // Left click opens the window, like on Windows; the menu is on right click.
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.send(Click::Show);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let item = |label: &str, click: Click| -> ksni::MenuItem<Self> {
            StandardItem {
                label: label.into(),
                activate: Box::new(move |t: &mut Self| {
                    let _ = t.tx.send(click);
                }),
                ..Default::default()
            }
            .into()
        };
        vec![
            item("Show YADM", Click::Show),
            item("New Download…", Click::Add),
            ksni::MenuItem::Separator,
            item("Pause All", Click::Pause),
            item("Resume All", Click::Resume),
            ksni::MenuItem::Separator,
            item("Quit YADM", Click::Quit),
        ]
    }
}

/// The glyph as ARGB32 in network byte order, which is what SNI expects.
fn icon() -> Vec<ksni::Icon> {
    let data = rgba(true).chunks_exact(4).flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
    vec![ksni::Icon { width: N as i32, height: N as i32, data }]
}

fn spawn() -> anyhow::Result<(Handle<YadmTray>, Receiver<Click>)> {
    let (tx, rx) = channel();
    let handle = YadmTray { tx, speed: String::new(), icon: icon() }.spawn()?;
    Ok((handle, rx))
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
        let mut tray: Option<(Handle<YadmTray>, Receiver<Click>)> = None;
        // Don't retry every tick when the desktop has no tray host.
        let mut failed = false;
        let mut last_speed = String::new();
        loop {
            cx.background_executor().timer(Duration::from_millis(400)).await;
            cx.update(|cx| {
                let s = store.read(cx);
                let want = s.settings().show_tray;
                let g = &s.snapshot().global;
                let speed = if g.active > 0 && g.dl_speed > 0 { format::speed(g.dl_speed) } else { String::new() };
                match (&tray, want) {
                    (None, true) if !failed => match spawn() {
                        Ok(t) => tray = Some(t),
                        Err(e) => {
                            failed = true;
                            tracing::warn!(
                                "tray icon unavailable (on GNOME, enable the AppIndicator extension): {e}"
                            );
                        }
                    },
                    (Some((h, _)), false) => {
                        h.shutdown();
                        tray = None;
                    }
                    (None, false) => failed = false,
                    _ => {}
                }
                // The tray host can go away (e.g. the extension is turned off).
                if tray.as_ref().is_some_and(|(h, _)| h.is_closed()) {
                    tray = None;
                }
                ACTIVE.store(tray.is_some(), Ordering::Relaxed);
                let Some((handle, rx)) = &tray else { return };
                if speed != last_speed {
                    handle.update(|t| t.speed = speed.clone());
                    last_speed = speed;
                }
                let clicks: Vec<Click> = rx.try_iter().collect();
                for click in clicks {
                    match click {
                        Click::Show => show_main_window(cx),
                        Click::Add => {
                            show_main_window(cx);
                            if let Some(w) = cx.windows().first() {
                                w.update(cx, |_, window, cx| window.dispatch_action(Box::new(NewDownload), cx)).ok();
                            }
                        }
                        Click::Pause => cx.dispatch_action(&PauseAll),
                        Click::Resume => cx.dispatch_action(&ResumeAll),
                        Click::Quit => cx.dispatch_action(&Quit),
                    }
                }
            });
        }
    })
    .detach();
}
