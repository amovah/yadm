//! ⌘K command palette.

use super::root::AppView;
use crate::actions::*;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::*;
use std::cell::Cell;

pub fn open(view: &mut AppView, window: &mut Window, cx: &mut Context<AppView>) {
    let state = cx.new(|cx| CommandState::new(window, cx));
    let downloads: Vec<(String, String)> = view
        .store
        .read(cx)
        .snapshot()
        .downloads
        .iter()
        .take(200)
        .map(|d| (d.id.clone(), d.name.clone()))
        .collect();
    let this = cx.entity().downgrade();
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            state.update(cx, |s, cx| s.focus(window, cx));
        }
        let item = |label: &str, icon: Lucide, action: Box<dyn Action>, keys: &[&str]| {
            CommandItem::new().label(label.to_string()).icon(icon).action(action).keywords(keys.iter().map(|k| k.to_string()))
        };
        let actions = CommandGroup::new().label("Actions").items([
            item("New download", Lucide::Plus, Box::new(NewDownload), &["add", "url", "link"]),
            item("Pause all", Lucide::Pause, Box::new(PauseAll), &["stop"]),
            item("Resume all", Lucide::Play, Box::new(ResumeAll), &["start", "continue"]),
            item("Clear completed", Lucide::ListX, Box::new(ClearFinished), &["clean", "remove"]),
            item("Open downloads folder", Lucide::FolderOpen, Box::new(OpenDownloadsFolder), &["finder"]),
            item("Switch theme", Lucide::SunMoon, Box::new(CycleTheme), &["dark", "light", "appearance"]),
            item("Settings", Lucide::Settings, Box::new(OpenSettings), &["preferences"]),
        ]);
        let nav = CommandGroup::new().label("Go to").items([
            item("All downloads", Lucide::Inbox, Box::new(ShowAll), &[]),
            item("Downloading", Lucide::CircleArrowDown, Box::new(ShowActive), &["active"]),
            item("Queued", Lucide::CircleDashed, Box::new(ShowQueued), &["waiting"]),
            item("Paused", Lucide::CirclePause, Box::new(ShowPaused), &[]),
            item("Completed", Lucide::CircleCheck, Box::new(ShowCompleted), &["done", "finished"]),
            item("Failed", Lucide::CircleX, Box::new(ShowFailed), &["error"]),
        ]);
        let ids: Vec<String> = downloads.iter().map(|(id, _)| id.clone()).collect();
        let items = CommandGroup::new()
            .label("Downloads")
            .items(downloads.iter().map(|(_, name)| CommandItem::new().label(name.clone()).icon(Lucide::File).action(Box::new(ToggleDetails))));
        let this = this.clone();
        dialog.w(px(560.)).close_button(false).child(
            Command::new(&state)
                .placeholder("Type a command or search downloads…")
                .group(actions)
                .group(nav)
                .group(items)
                .on_confirm(move |ix, window, cx| {
                    window.close_dialog(cx);
                    if ix.section == 2
                        && let Some(id) = ids.get(ix.row).cloned()
                    {
                        this.update(cx, |view, cx| {
                            view.cursor = Some(id);
                            view.detail_open = true;
                            view.request_details(cx);
                            cx.notify();
                        })
                        .ok();
                    }
                })
                .on_cancel(|window, cx| window.close_dialog(cx)),
        )
    });
}
