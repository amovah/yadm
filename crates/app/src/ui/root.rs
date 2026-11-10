//! The main window: sidebar, list (or settings), and the detail panel.

use crate::actions::*;
use crate::store::{AppStore, StoreEvent};
use crate::ui::add::AddDownload;
use crate::ui::settings::SettingsView;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::{BTreeSet, HashSet};
use std::time::Duration;
use yadm_core::{AddRequest, Category, Command, Download, State, ToastKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    All,
    State(State),
    Category(Category),
    Settings,
}

impl Nav {
    pub fn title(self) -> &'static str {
        match self {
            Nav::All => "All downloads",
            Nav::State(State::Active) => "Downloading",
            Nav::State(s) => s.label(),
            Nav::Category(c) => c.label(),
            Nav::Settings => "Settings",
        }
    }

    pub fn matches(self, d: &Download) -> bool {
        match self {
            Nav::All | Nav::Settings => true,
            Nav::State(s) => d.state == s,
            Nav::Category(c) => d.category == c,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    Status,
    Category,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GroupKey {
    State(State),
    Category(Category),
}

pub enum Row {
    Header { key: GroupKey, count: usize },
    Item(usize),
}

pub struct AppView {
    pub(super) store: Entity<AppStore>,
    pub(super) focus: FocusHandle,
    pub(super) nav: Nav,
    pub(super) group_by: GroupBy,
    pub(super) collapsed: HashSet<GroupKey>,
    pub(super) search: Entity<InputState>,
    pub(super) query: String,
    pub(super) rows: Vec<Row>,
    /// Ids in visual order (for keyboard navigation and range selection).
    pub(super) order: Vec<String>,
    pub(super) cursor: Option<String>,
    pub(super) selected: BTreeSet<String>,
    pub(super) detail_open: bool,
    pub(super) detail_tab: usize,
    pub(super) scroll: UniformListScrollHandle,
    pub(super) settings_view: Option<Entity<SettingsView>>,
    pub(super) last_clipboard: Option<String>,
    _subscriptions: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}

impl AppView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = AppStore::global(cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Filter by name or host…"));
        let mut subs = vec![];
        subs.push(cx.observe(&store, |this, _, cx| {
            this.rebuild_rows(cx);
            cx.notify();
        }));
        subs.push(cx.subscribe_in(&store, window, Self::on_store_event));
        subs.push(cx.subscribe_in(&search, window, |this, state, ev, window, cx| match ev {
            InputEvent::Change => {
                this.query = state.read(cx).value().to_string();
                this.rebuild_rows(cx);
                cx.notify();
            }
            InputEvent::PressEnter { .. } => this.focus.focus(window, cx),
            _ => {}
        }));
        subs.push(cx.observe_window_appearance(window, |_, _, cx| {
            let pref = AppStore::global(cx).read(cx).settings().theme;
            crate::theme::apply(pref, cx);
        }));
        subs.push(cx.observe_window_activation(window, |this, window, cx| {
            let visible = window.is_window_active();
            this.store.read(cx).send(Command::SetVisible(visible));
        }));

        // Periodic work: refresh details of the open item and watch the clipboard.
        let tick = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(1500)).await;
                if this.update_in(cx, |this, window, cx| this.tick(window, cx)).is_err() {
                    break;
                }
            }
        });

        let mut this = Self {
            store,
            focus: cx.focus_handle(),
            nav: Nav::All,
            group_by: GroupBy::Status,
            collapsed: HashSet::new(),
            search,
            query: String::new(),
            rows: vec![],
            order: vec![],
            cursor: None,
            selected: BTreeSet::new(),
            detail_open: false,
            detail_tab: 0,
            scroll: UniformListScrollHandle::new(),
            settings_view: None,
            last_clipboard: None,
            _subscriptions: subs,
            _tasks: vec![tick],
        };
        // Don't offer whatever is already on the clipboard at launch.
        this.last_clipboard = cx.read_from_clipboard().and_then(|c| c.text());
        this.rebuild_rows(cx);
        this.focus.focus(window, cx);
        this
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.detail_open
            && let Some(id) = self.cursor.clone()
        {
            self.store.read(cx).send(Command::RequestDetails(id));
        }
        self.watch_clipboard(window, cx);
    }

    fn watch_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.store.read(cx).settings().clipboard_monitor {
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else { return };
        if self.last_clipboard.as_deref() == Some(text.as_str()) {
            return;
        }
        self.last_clipboard = Some(text.clone());
        let links = crate::clipboard::download_links(&text);
        if links.is_empty() {
            return;
        }
        let known: HashSet<&str> = self
            .store
            .read(cx)
            .snapshot()
            .downloads
            .iter()
            .filter_map(|d| d.uris.first().map(String::as_str))
            .collect();
        let links: Vec<String> = links.into_iter().filter(|l| !known.contains(l.as_str())).collect();
        if links.is_empty() {
            return;
        }
        let title = if links.len() == 1 {
            format!("Download {}?", crate::clipboard::short_name(&links[0]))
        } else {
            format!("Download {} copied links?", links.len())
        };
        let view = cx.entity().downgrade();
        let note = Notification::new()
            .title("Link copied")
            .message(title)
            .action(move |_, _, _| {
                let view = view.clone();
                let links = links.clone();
                gpui_kit::component::button::Button::new("clip-add")
                    .label("Add")
                    .on_click(move |_, window, cx| {
                        let req = AddRequest { uris: links.clone(), ..Default::default() };
                        view.update(cx, |this, cx| this.open_add(Some(req), window, cx)).ok();
                    })
            });
        window.push_notification(note, cx);
    }

    fn on_store_event(&mut self, _: &Entity<AppStore>, ev: &StoreEvent, window: &mut Window, cx: &mut Context<Self>) {
        let note = match ev {
            StoreEvent::Toast(kind, msg) => match kind {
                ToastKind::Info => Notification::info(msg.clone()),
                ToastKind::Success => Notification::success(msg.clone()),
                ToastKind::Error => Notification::error(msg.clone()),
            },
            StoreEvent::Completed { id, name } => {
                if !self.store.read(cx).settings().notify_on_complete {
                    return;
                }
                let id = id.clone();
                let mut n = Notification::success(name.clone()).title("Download complete").action(move |_, _, _| {
                    let id = id.clone();
                    gpui_kit::component::button::Button::new("reveal").label("Show in Finder").on_click(move |_, _, cx| {
                        let store = AppStore::global(cx);
                        if let Some(p) = store.read(cx).download(&id).and_then(|d| d.local_path()) {
                            cx.reveal_path(&p);
                        }
                    })
                });
                if !window.is_window_active() && crate::is_bundled() {
                    n = n.in_app_and_system();
                }
                n
            }
            StoreEvent::Failed { name, error } => Notification::error(error.clone()).title(format!("{name} failed")),
            StoreEvent::BrowserRequest(req) => {
                cx.activate(true);
                window.activate_window();
                self.open_add(Some(req.clone()), window, cx);
                return;
            }
        };
        window.push_notification(note, cx);
    }

    // ---- derived rows -----------------------------------------------------

    pub(super) fn visible_downloads<'a>(&self, all: &'a [Download]) -> Vec<(usize, &'a Download)> {
        let q = self.query.trim().to_lowercase();
        all.iter()
            .enumerate()
            .filter(|(_, d)| self.nav.matches(d))
            .filter(|(_, d)| {
                q.is_empty()
                    || d.name.to_lowercase().contains(&q)
                    || d.host().is_some_and(|h| h.to_lowercase().contains(&q))
                    || d.uris.first().is_some_and(|u| u.to_lowercase().contains(&q))
            })
            .collect()
    }

    pub(super) fn rebuild_rows(&mut self, cx: &mut Context<Self>) {
        let snap = self.store.read(cx).snapshot().clone();
        let mut items = self.visible_downloads(&snap.downloads);
        // Queued items follow aria2's queue order; others newest first.
        items.sort_by_key(|(i, d)| (d.queue_pos.unwrap_or(usize::MAX), *i));
        if !items.iter().any(|(_, d)| d.queue_pos.is_some()) {
            items.sort_by_key(|(i, _)| *i);
        }

        let mut rows = vec![];
        let mut order = vec![];
        let mut push_group = |key: GroupKey, members: Vec<(usize, &Download)>, rows: &mut Vec<Row>| {
            if members.is_empty() {
                return;
            }
            rows.push(Row::Header { key, count: members.len() });
            if !self.collapsed.contains(&key) {
                for (i, d) in members {
                    rows.push(Row::Item(i));
                    order.push(d.id.clone());
                }
            }
        };
        match self.group_by {
            GroupBy::Status => {
                for s in State::GROUPS {
                    let m = items.iter().filter(|(_, d)| d.state == s).cloned().collect();
                    push_group(GroupKey::State(s), m, &mut rows);
                }
            }
            GroupBy::Category => {
                for c in Category::ALL {
                    let m = items.iter().filter(|(_, d)| d.category == c).cloned().collect();
                    push_group(GroupKey::Category(c), m, &mut rows);
                }
            }
            GroupBy::None => {
                for (i, d) in &items {
                    rows.push(Row::Item(*i));
                    order.push(d.id.clone());
                }
            }
        }
        self.rows = rows;
        // Drop selections that disappeared.
        let alive: HashSet<&String> = snap.downloads.iter().map(|d| &d.id).collect();
        self.selected.retain(|id| alive.contains(id));
        if self.cursor.as_ref().is_some_and(|c| !alive.contains(c)) {
            self.cursor = None;
            self.detail_open = false;
        }
        self.order = order;
    }

    pub(super) fn set_nav(&mut self, nav: Nav, window: &mut Window, cx: &mut Context<Self>) {
        self.nav = nav;
        if nav == Nav::Settings {
            if self.settings_view.is_none() {
                self.settings_view = Some(cx.new(|cx| SettingsView::new(window, cx)));
            }
        } else {
            self.focus.focus(window, cx);
        }
        self.selected.clear();
        self.rebuild_rows(cx);
        cx.notify();
    }

    /// Ids targeted by an action: the multi-selection, else the cursor row.
    pub(super) fn targets(&self) -> Vec<String> {
        if !self.selected.is_empty() {
            self.selected.iter().cloned().collect()
        } else {
            self.cursor.iter().cloned().collect()
        }
    }

    pub fn open_add(&mut self, req: Option<AddRequest>, window: &mut Window, cx: &mut Context<Self>) {
        AddDownload::open(req, window, cx);
    }

    // ---- actions ----------------------------------------------------------

    fn move_cursor(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.order.is_empty() {
            return;
        }
        let cur = self.cursor.as_ref().and_then(|c| self.order.iter().position(|o| o == c));
        let next = match cur {
            None => 0,
            Some(i) => (i as isize + delta).clamp(0, self.order.len() as isize - 1) as usize,
        };
        let id = self.order[next].clone();
        if let Some(row) = self.rows.iter().position(|r| matches!(r, Row::Item(i) if self.store.read(cx).snapshot().downloads.get(*i).is_some_and(|d| d.id == id))) {
            self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
        }
        self.cursor = Some(id);
        if self.detail_open {
            self.request_details(cx);
        }
        cx.notify();
    }

    pub(super) fn request_details(&self, cx: &mut Context<Self>) {
        if let Some(id) = &self.cursor {
            self.store.read(cx).send(Command::RequestDetails(id.clone()));
        }
    }

    pub(super) fn confirm_remove(&mut self, ids: Vec<String>, delete_files: bool, window: &mut Window, cx: &mut Context<Self>) {
        if ids.is_empty() {
            return;
        }
        let store = self.store.clone();
        let snapshot = store.read(cx).snapshot().clone();
        let names: Vec<_> = ids.iter().filter_map(|id| snapshot.downloads.iter().find(|d| &d.id == id)).map(|d| d.name.clone()).collect();
        let title = if names.len() == 1 { format!("Remove “{}”?", names[0]) } else { format!("Remove {} downloads?", names.len()) };
        let finished_only = ids.iter().all(|id| snapshot.downloads.iter().any(|d| &d.id == id && !d.state.is_live()));
        if !delete_files && finished_only {
            // Removing finished items from the list is harmless; don't ask.
            store.read(cx).send(Command::Remove { ids, delete_files: false });
            return;
        }
        let description = if delete_files {
            "The downloaded files will be moved to the Trash."
        } else {
            "Transfers stop and partial files stay on disk."
        };
        window.open_alert_dialog(cx, move |alert, _, _| {
            let ids = ids.clone();
            let store = store.clone();
            alert
                .title(title.clone())
                .description(description)
                .show_cancel(true)
                .ok_text(if delete_files { "Move to Trash" } else { "Remove" })
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .on_ok(move |_, _, cx| {
                    store.read(cx).send(Command::Remove { ids: ids.clone(), delete_files });
                    true
                })
        });
    }

    fn with_cursor_download(&self, cx: &App, f: impl FnOnce(&Download)) {
        if let Some(d) = self.cursor.as_ref().and_then(|id| self.store.read(cx).download(id)) {
            f(d);
        }
    }

    fn register_actions(&mut self, el: Div, cx: &mut Context<Self>) -> Div {
        el.on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_cursor(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_cursor(-1, cx)))
            .on_action(cx.listener(|this, _: &ToggleSelect, _, cx| {
                if let Some(id) = this.cursor.clone()
                    && !this.selected.remove(&id)
                {
                    this.selected.insert(id);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.selected = this.order.iter().cloned().collect();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleDetails, _, cx| {
                if this.cursor.is_none() {
                    this.cursor = this.order.first().cloned();
                }
                this.detail_open = this.cursor.is_some() && !this.detail_open;
                this.request_details(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CloseOverlay, window, cx| {
                if !this.selected.is_empty() {
                    this.selected.clear();
                } else if this.detail_open {
                    this.detail_open = false;
                } else if !this.query.is_empty() {
                    this.search.update(cx, |s, cx| s.set_value("", window, cx));
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PauseResume, _, cx| {
                let store = this.store.read(cx);
                let ids = this.targets();
                let any_running = ids.iter().filter_map(|id| store.download(id)).any(|d| matches!(d.state, State::Active | State::Queued));
                store.send(if any_running { Command::Pause(ids) } else { Command::Resume(ids) });
            }))
            .on_action(cx.listener(|this, _: &RetrySelected, _, cx| {
                this.store.read(cx).send(Command::Retry(this.targets()));
            }))
            .on_action(cx.listener(|this, _: &RemoveSelected, window, cx| this.confirm_remove(this.targets(), false, window, cx)))
            .on_action(cx.listener(|this, _: &RemoveSelectedWithFiles, window, cx| this.confirm_remove(this.targets(), true, window, cx)))
            .on_action(cx.listener(|this, _: &RevealSelected, _, cx| {
                this.with_cursor_download(cx, |d| {
                    let p = d.local_path().unwrap_or_else(|| d.dir.clone().into());
                    cx.reveal_path(&p);
                });
            }))
            .on_action(cx.listener(|this, _: &OpenSelected, _, cx| {
                this.with_cursor_download(cx, |d| {
                    if d.state == State::Completed
                        && let Some(p) = d.local_path()
                    {
                        cx.open_with_system(&p);
                    }
                });
            }))
            .on_action(cx.listener(|this, _: &CopySourceUrl, window, cx| {
                let mut url = None;
                this.with_cursor_download(cx, |d| url = d.uris.first().cloned());
                if let Some(u) = url {
                    cx.write_to_clipboard(ClipboardItem::new_string(u));
                    window.push_notification(Notification::info("Source URL copied"), cx);
                }
            }))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| {
                if let Some(id) = this.cursor.clone() {
                    this.store.read(cx).send(Command::Move { id, pos: -1, absolute: false });
                }
            }))
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| {
                if let Some(id) = this.cursor.clone() {
                    this.store.read(cx).send(Command::Move { id, pos: 1, absolute: false });
                }
            }))
            .on_action(cx.listener(|this, _: &PasteDownload, window, cx| {
                let text = cx.read_from_clipboard().and_then(|c| c.text()).unwrap_or_default();
                let links = crate::clipboard::download_links(&text);
                let req = (!links.is_empty()).then(|| AddRequest { uris: links, ..Default::default() });
                this.open_add(req, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                if this.nav == Nav::Settings {
                    this.set_nav(Nav::All, window, cx);
                }
                this.search.update(cx, |s, cx| s.focus(window, cx));
            }))
    }

    fn register_global_actions(&mut self, el: Div, cx: &mut Context<Self>) -> Div {
        el.on_action(cx.listener(|this, _: &NewDownload, window, cx| this.open_add(None, window, cx)))
            .on_action(cx.listener(|this, _: &OpenCommandPalette, window, cx| crate::ui::palette::open(this, window, cx)))
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| this.set_nav(Nav::Settings, window, cx)))
            .on_action(cx.listener(|this, _: &ShowAll, window, cx| this.set_nav(Nav::All, window, cx)))
            .on_action(cx.listener(|this, _: &ShowActive, window, cx| this.set_nav(Nav::State(State::Active), window, cx)))
            .on_action(cx.listener(|this, _: &ShowQueued, window, cx| this.set_nav(Nav::State(State::Queued), window, cx)))
            .on_action(cx.listener(|this, _: &ShowPaused, window, cx| this.set_nav(Nav::State(State::Paused), window, cx)))
            .on_action(cx.listener(|this, _: &ShowCompleted, window, cx| this.set_nav(Nav::State(State::Completed), window, cx)))
            .on_action(cx.listener(|this, _: &ShowFailed, window, cx| this.set_nav(Nav::State(State::Failed), window, cx)))
            .on_action(cx.listener(|this, _: &PauseAll, _, cx| this.store.read(cx).send(Command::PauseAll)))
            .on_action(cx.listener(|this, _: &ResumeAll, _, cx| this.store.read(cx).send(Command::ResumeAll)))
            .on_action(cx.listener(|this, _: &ClearFinished, _, cx| this.store.read(cx).send(Command::ClearFinished)))
            .on_action(cx.listener(|this, _: &OpenDownloadsFolder, _, cx| {
                let dir = this.store.read(cx).settings().download_dir.clone();
                cx.open_with_system(&dir);
            }))
            .on_action(cx.listener(|this, _: &CycleTheme, _, cx| {
                this.store.update(cx, |store, cx| {
                    let mut s = store.settings().clone();
                    s.theme = match s.theme {
                        yadm_core::ThemePref::System => yadm_core::ThemePref::Dark,
                        yadm_core::ThemePref::Dark => yadm_core::ThemePref::Light,
                        yadm_core::ThemePref::Light => yadm_core::ThemePref::System,
                    };
                    crate::theme::apply(s.theme, cx);
                    store.update_settings(s, cx);
                });
            }))
    }
}

impl Focusable for AppView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main: AnyElement = match (&self.nav, &self.settings_view) {
            (Nav::Settings, Some(view)) => view.clone().into_any_element(),
            _ => {
                let list = self.render_list(window, cx);
                let list = self.register_actions(list, cx);
                list.into_any_element()
            }
        };
        let show_detail = self.detail_open && self.nav != Nav::Settings;
        let root = h_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().foreground)
            .text_size(px(13.))
            .child(self.render_sidebar(window, cx))
            .child(
                // Linear's inset content panel.
                div().flex_1().min_w_0().h_full().py_2().pr_2().child(
                    h_flex()
                        .size_full()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().background)
                        .overflow_hidden()
                        .child(div().flex_1().min_w_0().h_full().child(main))
                        .when(show_detail, |this| this.child(self.render_detail(window, cx))),
                ),
            )
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                let torrents: Vec<_> = paths.paths().iter().filter(|p| p.extension().is_some_and(|e| e == "torrent" || e == "metalink" || e == "meta4")).cloned().collect();
                for p in torrents {
                    match std::fs::read(&p) {
                        Ok(bytes) => {
                            let metalink = p.extension().is_some_and(|e| e != "torrent");
                            this.store.read(cx).send(Command::Add(AddRequest { torrent: Some(bytes), metalink, ..Default::default() }));
                        }
                        Err(e) => window.push_notification(Notification::error(format!("Couldn't read {}: {e}", p.display())), cx),
                    }
                }
            }));
        self.register_global_actions(root, cx)
    }
}
