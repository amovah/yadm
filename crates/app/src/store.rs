//! The UI-side mirror of engine state. One entity per app; views observe it.

use gpui_kit::{App, AppContext as _, AsyncApp, Context, Entity, EventEmitter, Global};
use std::sync::Arc;
use yadm_core::{AddRequest, Command, Download, EngineHandle, Event, Settings, Snapshot, ToastKind};

pub enum StoreEvent {
    Toast(ToastKind, String),
    Completed { id: String, name: String },
    Failed { name: String, error: String },
    BrowserRequest(AddRequest),
    /// A browser extension asks to pair (see [`yadm_core::Command::AnswerPair`]).
    PairRequest { id: u64, browser: String },
}

pub struct AppStore {
    engine: EngineHandle,
    snapshot: Arc<Snapshot>,
    settings: Settings,
}

impl EventEmitter<StoreEvent> for AppStore {}

/// Global handle so any view can reach the store.
pub struct GlobalStore(pub Entity<AppStore>);
impl Global for GlobalStore {}

impl AppStore {
    pub fn new(engine: EngineHandle, settings: Settings) -> Self {
        Self { engine, snapshot: Arc::new(Snapshot::default()), settings }
    }

    pub fn global(cx: &App) -> Entity<AppStore> {
        cx.global::<GlobalStore>().0.clone()
    }

    pub fn snapshot(&self) -> &Arc<Snapshot> {
        &self.snapshot
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn download(&self, id: &str) -> Option<&Download> {
        self.snapshot.downloads.iter().find(|d| d.id == id)
    }

    pub fn send(&self, cmd: Command) {
        self.engine.send(cmd);
    }

    pub fn engine(&self) -> &EngineHandle {
        &self.engine
    }

    pub fn update_settings(&mut self, settings: Settings, cx: &mut Context<Self>) {
        if settings == self.settings {
            return;
        }
        self.settings = settings.clone();
        self.engine.send(Command::UpdateSettings(Box::new(settings)));
        cx.notify();
    }

    fn apply(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Snapshot(s) => {
                self.snapshot = s;
                cx.notify();
            }
            Event::Details(_) => {}
            Event::Completed { id, name } => cx.emit(StoreEvent::Completed { id, name }),
            Event::Failed { name, error, .. } => cx.emit(StoreEvent::Failed { name, error }),
            Event::Toast { kind, message } => cx.emit(StoreEvent::Toast(kind, message)),
            Event::BrowserRequest(req) => cx.emit(StoreEvent::BrowserRequest(req)),
            Event::PairRequest { id, browser } => cx.emit(StoreEvent::PairRequest { id, browser }),
            Event::SettingsChanged(s) => {
                self.settings = *s;
                cx.notify();
            }
        }
    }

    /// Pumps engine events into the store on the foreground executor.
    pub fn start_pump(store: &Entity<AppStore>, cx: &mut App) {
        let events = store.read(cx).engine.events.clone();
        let store = store.downgrade();
        cx.spawn(async move |cx: &mut AsyncApp| {
            while let Ok(ev) = events.recv().await {
                // Coalesce bursts of snapshots: only the latest matters.
                let mut batch = vec![ev];
                while let Ok(more) = events.try_recv() {
                    batch.push(more);
                }
                let mut last_snapshot = None;
                let mut others = vec![];
                for e in batch {
                    match e {
                        Event::Snapshot(s) => last_snapshot = Some(s),
                        e => others.push(e),
                    }
                }
                let ok = store.update(cx, |store, cx| {
                    for e in others {
                        store.apply(e, cx);
                    }
                    if let Some(s) = last_snapshot {
                        store.apply(Event::Snapshot(s), cx);
                    }
                });
                if ok.is_err() {
                    break;
                }
            }
        })
        .detach();
    }
}

pub fn new_store(engine: EngineHandle, settings: Settings, cx: &mut App) -> Entity<AppStore> {
    let store = cx.new(|_| AppStore::new(engine, settings));
    cx.set_global(GlobalStore(store.clone()));
    AppStore::start_pump(&store, cx);
    store
}
