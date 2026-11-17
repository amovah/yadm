//! YADM — a Linear-style download manager on gpui + aria2.

// No console window behind the app in Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod actions;
mod autostart;
mod clipboard;
mod platform;
mod store;
mod theme;
#[cfg(any(target_os = "macos", windows))]
mod tray;
#[cfg(target_os = "linux")]
#[path = "tray_linux.rs"]
mod tray;
/// No tray elsewhere (see [`platform::HAS_TRAY`]): closing the window quits.
#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
mod tray {
    pub fn start(_: &mut gpui_kit::App) {}
    pub fn is_active() -> bool {
        false
    }
}
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
mod tray_glyph;
mod ui;

use actions::*;
use gpui_kit::component::TitleBar;
use gpui_kit::*;
use std::time::Duration;
use store::{AppStore, StoreEvent};
use ui::AppView;
use ui::kit::dp;
use yadm_core::{Engine, Paths, Settings};

pub fn open_main_window(cx: &mut App) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(dp(1260.), dp(800.)), cx)),
        window_min_size: Some(size(dp(880.), dp(520.))),
        titlebar: Some(TitlebarOptions {
            traffic_light_position: Some(point(px(16.), px(14.))),
            // The window manager shows this, e.g. in GNOME's "“YADM” is ready".
            title: Some("YADM".into()),
            ..TitleBar::title_bar_options()
        }),
        app_id: Some("dev.yadm.app".into()),
        ..Default::default()
    };
    let result = gpui_kit::open_window(options, cx, |window, cx| {
        window.on_window_should_close(cx, should_close_window);
        cx.new(|cx| AppView::new(window, cx))
    });
    if let Err(e) = result {
        tracing::error!("failed to open window: {e:#}");
    }
}

/// Close-button policy. With "close to tray" the app keeps downloading in the
/// background: macOS hides it (the Dock and menu bar bring it back), other
/// platforms close the window and the tray icon reopens it. Returns whether
/// the window should actually close.
pub fn should_close_window(window: &mut Window, cx: &mut App) -> bool {
    // This runs inside the window's own update, where dispatching an action
    // can't re-enter the window ("window not found"); quit once it's done.
    if !AppStore::global(cx).read(cx).settings().close_to_tray {
        cx.defer(quit);
        return false;
    }
    if platform::IS_MAC {
        window.minimize_window();
        cx.hide();
        false
    } else if tray::is_active() {
        true
    } else {
        // No tray icon to come back through: quitting beats an invisible app.
        cx.defer(quit);
        false
    }
}

/// ⌘W / Ctrl+W: put the window away without quitting.
fn hide_window(cx: &mut App) {
    if platform::IS_MAC {
        cx.hide();
        return;
    }
    let Some(w) = cx.active_window().or_else(|| cx.windows().first().copied()) else { return };
    let to_tray = tray::is_active();
    // Actions run while the window is being updated; touch it afterwards.
    cx.defer(move |cx| {
        w.update(cx, |_, window, _| if to_tray { window.remove_window() } else { window.minimize_window() }).ok();
    });
}

/// Login start on Windows/Linux: close the window into the tray once the
/// tray is up. Without one (no StatusNotifierItem host, or the icon turned
/// off) the window stays, rather than leaving YADM running out of reach.
fn start_in_tray(cx: &mut App) {
    cx.spawn(async |cx: &mut AsyncApp| {
        // The tray host may come up a little after the session starts.
        for _ in 0..30 {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            let done = cx.update(|cx| {
                if !AppStore::global(cx).read(cx).settings().show_tray {
                    return true;
                }
                if !tray::is_active() {
                    return false;
                }
                for w in cx.windows() {
                    w.update(cx, |_, window, _| window.remove_window()).ok();
                }
                true
            });
            if done {
                break;
            }
        }
    })
    .detach();
}

/// Browser hand-offs and pairing are answered in the main window. If it's
/// closed into the tray, open it and replay the request to the new window.
fn reopen_for_browser_requests(cx: &mut App) {
    let store = AppStore::global(cx);
    cx.subscribe(&store, |store, ev, cx| {
        if !cx.windows().is_empty() {
            return;
        }
        let replay = match ev {
            StoreEvent::BrowserRequest(req) => StoreEvent::BrowserRequest(req.clone()),
            StoreEvent::PairRequest { id, browser } => StoreEvent::PairRequest { id: *id, browser: browser.clone() },
            _ => return,
        };
        cx.defer(move |cx| {
            open_main_window(cx);
            store.update(cx, |_, cx| cx.emit(replay));
        });
    })
    .detach();
}

fn quit(cx: &mut App) {
    let engine = AppStore::global(cx).read(cx).engine().clone();
    engine.shutdown_blocking(Duration::from_secs(4));
    cx.quit();
}

fn set_menus(cx: &mut App) {
    cx.set_menus([
        Menu {
            name: "YADM".into(),
            items: vec![
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("Hide YADM", HideWindow),
                MenuItem::action("Quit YADM", Quit),
            ],
            disabled: false,
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("New Download…", NewDownload),
                MenuItem::action("Command Palette…", OpenCommandPalette),
                MenuItem::separator(),
                MenuItem::action("Pause All", PauseAll),
                MenuItem::action("Resume All", ResumeAll),
                MenuItem::action("Clear Completed", ClearFinished),
            ],
            disabled: false,
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("All Downloads", ShowAll),
                MenuItem::action("Downloading", ShowActive),
                MenuItem::action("Queued", ShowQueued),
                MenuItem::action("Paused", ShowPaused),
                MenuItem::action("Completed", ShowCompleted),
                MenuItem::action("Failed", ShowFailed),
                MenuItem::separator(),
                MenuItem::action("Switch Theme", CycleTheme),
            ],
            disabled: false,
        },
    ]);
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    #[cfg(target_os = "linux")]
    platform::register_desktop_entry();

    let paths = Paths::system();
    let settings = Settings::load(&paths.settings());
    autostart::sync(settings.launch_at_login);
    // Launched by the login item: start in the background.
    let hidden = std::env::args().skip(1).any(|a| a == autostart::HIDDEN_ARG);
    let engine = Engine::start(paths, settings.clone());

    // The tray keeps YADM alive with no window open.
    let app = gpui_kit::application().with_assets(gpui_kit::assets::AllAssets).with_quit_mode(QuitMode::Explicit);
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_main_window(cx);
        }
        cx.activate(true);
    });
    app.run(move |cx| {
        gpui_kit::init(cx);
        ui::kit::set_ui_scale(settings.ui_scale);
        theme::init(settings.theme, cx);
        actions::bind(cx);
        ui::add::bind(cx);
        store::new_store(engine, settings, cx);

        cx.on_action(|_: &Quit, cx| quit(cx));
        cx.on_action(|_: &HideWindow, cx| hide_window(cx));
        set_menus(cx);

        open_main_window(cx);
        tray::start(cx);
        if !hidden {
            cx.activate(true);
        } else if platform::IS_MAC {
            // Like closing to the menu bar: the window stays (and with it
            // notifications and browser requests); the Dock brings it back.
            cx.hide();
        } else {
            start_in_tray(cx);
        }
        reopen_for_browser_requests(cx);
    });
}
