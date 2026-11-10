//! YADM — a Linear-style download manager on gpui + aria2.

mod actions;
mod clipboard;
mod store;
mod theme;
mod tray;
mod ui;

use actions::*;
use gpui_kit::component::TitleBar;
use gpui_kit::*;
use std::path::PathBuf;
use std::time::Duration;
use store::AppStore;
use ui::AppView;
use yadm_core::{Engine, Paths, Settings};

/// True when running from a `.app` bundle (system notifications need one).
pub fn is_bundled() -> bool {
    std::env::current_exe().is_ok_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS"))
}

/// Where the unpacked browser extension lives: inside the bundle, else the repo.
pub fn extension_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(res) = exe.parent().and_then(|p| p.parent()).map(|p| p.join("Resources/extension"))
        && res.exists()
    {
        return res;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extension")
}

pub fn open_main_window(cx: &mut App) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1260.), px(800.)), cx)),
        window_min_size: Some(size(px(880.), px(520.))),
        titlebar: Some(TitlebarOptions {
            traffic_light_position: Some(point(px(16.), px(14.))),
            ..TitleBar::title_bar_options()
        }),
        app_id: Some("dev.yadm.app".into()),
        ..Default::default()
    };
    let result = gpui_kit::open_window(options, cx, |window, cx| {
        window.on_window_should_close(cx, |window, cx| {
            if AppStore::global(cx).read(cx).settings().close_to_tray {
                // Keep downloading in the background; the menu bar item brings it back.
                window.minimize_window();
                cx.hide();
                false
            } else {
                cx.dispatch_action(&Quit);
                false
            }
        });
        cx.new(|cx| AppView::new(window, cx))
    });
    if let Err(e) = result {
        tracing::error!("failed to open window: {e:#}");
    }
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

    let paths = Paths::system();
    let settings = Settings::load(&paths.settings());
    let engine = Engine::start(paths, settings.clone());

    let app = gpui_kit::application().with_assets(gpui_kit::assets::AllAssets);
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            open_main_window(cx);
        }
        cx.activate(true);
    });
    app.run(move |cx| {
        gpui_kit::init(cx);
        theme::init(settings.theme, cx);
        actions::bind(cx);
        ui::add::bind(cx);
        store::new_store(engine, settings, cx);

        cx.on_action(|_: &Quit, cx| quit(cx));
        cx.on_action(|_: &HideWindow, cx| cx.hide());
        set_menus(cx);

        open_main_window(cx);
        tray::start(cx);
        cx.activate(true);
    });
}
