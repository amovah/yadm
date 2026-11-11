//! The engine actor: supervises aria2c, mirrors its state into [`Download`]s,
//! applies schedules and serves the browser bridge.

use crate::category::Category;
use crate::model::*;
use crate::paths::Paths;
use crate::schedule::{self, Effective};
use crate::settings::{BridgeSettings, Settings};
use crate::store::Store;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use yadm_aria2::{
    Client, DownloadStatus, LIST_KEYS, NotificationKind, Options, Sidecar, SidecarConfig, Status,
};
use yadm_bridge::{Bridge, BridgeConfig, BridgeMessage, BrowserRequest};

/// Handle held by the UI. Cloneable; dropping all handles does not stop the
/// engine — send [`Command::Shutdown`].
#[derive(Clone)]
pub struct EngineHandle {
    commands: async_channel::Sender<Command>,
    pub events: async_channel::Receiver<Event>,
}

impl EngineHandle {
    pub fn send(&self, cmd: Command) {
        let _ = self.commands.try_send(cmd);
    }

    /// Sends `Shutdown` and waits (bounded) for the engine to stop aria2 cleanly.
    pub fn shutdown_blocking(&self, timeout: Duration) {
        let (tx, rx) = std::sync::mpsc::channel();
        let commands = self.commands.clone();
        std::thread::spawn(move || {
            let _ = commands.send_blocking(Command::Shutdown);
            // The engine closes the event channel when done.
            let _ = tx.send(());
        });
        let _ = rx.recv_timeout(timeout);
        let deadline = Instant::now() + timeout;
        while !self.events.is_closed() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

pub struct Engine;

impl Engine {
    /// Starts the engine on its own thread with a tokio runtime.
    pub fn start(paths: Paths, settings: Settings) -> EngineHandle {
        let (cmd_tx, cmd_rx) = async_channel::unbounded();
        let (ev_tx, ev_rx) = async_channel::unbounded();
        std::thread::Builder::new()
            .name("yadm-engine".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                rt.block_on(async move {
                    match Actor::new(paths, settings, ev_tx.clone()) {
                        Ok(actor) => actor.run(cmd_rx).await,
                        Err(e) => {
                            let snap = Snapshot {
                                engine: EngineStatus::Failed(format!("{e:#}")),
                                ..Default::default()
                            };
                            let _ = ev_tx.send(Event::Snapshot(Arc::new(snap))).await;
                            while let Ok(cmd) = cmd_rx.recv().await {
                                if matches!(cmd, Command::Shutdown) {
                                    break;
                                }
                            }
                        }
                    }
                    ev_tx.close();
                });
            })
            .expect("spawn engine thread");
        EngineHandle {
            commands: cmd_tx,
            events: ev_rx,
        }
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn s(v: impl ToString) -> Value {
    Value::String(v.to_string())
}

/// File name guessed from a URL's last path segment.
pub(crate) fn name_from_uri(uri: &str) -> String {
    if uri.starts_with("magnet:") {
        if let Ok(u) = url::Url::parse(uri) {
            if let Some((_, dn)) = u.query_pairs().find(|(k, _)| k == "dn") {
                return dn.into_owned();
            }
            if let Some((_, xt)) = u.query_pairs().find(|(k, _)| k == "xt") {
                let hash = xt.rsplit(':').next().unwrap_or_default().to_string();
                return format!("Magnet {}", &hash[..hash.len().min(12)]);
            }
        }
        return "Magnet link".into();
    }
    let Ok(u) = url::Url::parse(uri) else {
        return uri.to_string();
    };
    u.path_segments()
        .and_then(|mut s| s.next_back())
        .filter(|s| !s.is_empty())
        .map(|s| {
            percent_encoding::percent_decode_str(s)
                .decode_utf8_lossy()
                .into_owned()
        })
        .or_else(|| u.host_str().map(str::to_owned))
        .unwrap_or_else(|| uri.to_string())
}

struct Actor {
    paths: Paths,
    settings: Settings,
    store: Store,
    ev: async_channel::Sender<Event>,

    downloads: Vec<Download>,
    by_gid: HashMap<String, DownloadId>,
    global: GlobalStats,
    engine: EngineStatus,

    client: Option<Client>,
    applied: Effective,
    schedule_label: Option<String>,
    /// gids paused by the scheduler (resumed when its window ends).
    schedule_paused: HashSet<String>,

    dirty: HashSet<DownloadId>,
    last_flush: Instant,
    visible: bool,

    bridge: Option<(Bridge, BridgeSettings)>,
    bridge_tx: async_channel::Sender<BridgeMessage>,
    bridge_rx: async_channel::Receiver<BridgeMessage>,
    /// Extension pairing requests waiting for the user's answer.
    pending_pairs: HashMap<u64, tokio::sync::oneshot::Sender<bool>>,
    next_pair: u64,
}

enum Flow {
    Restart,
    Shutdown,
}

impl Actor {
    fn new(
        paths: Paths,
        settings: Settings,
        ev: async_channel::Sender<Event>,
    ) -> anyhow::Result<Self> {
        let store = Store::open(&paths.database())?;
        let mut downloads = store.all()?;
        for d in &mut downloads {
            // Nothing is transferring until aria2 reports in.
            d.dl_speed = 0;
            d.ul_speed = 0;
        }
        let by_gid = downloads
            .iter()
            .filter_map(|d| Some((d.gid.clone()?, d.id.clone())))
            .collect();
        let (bridge_tx, bridge_rx) = async_channel::unbounded();
        Ok(Self {
            paths,
            settings,
            store,
            ev,
            downloads,
            by_gid,
            global: GlobalStats::default(),
            engine: EngineStatus::Starting,
            client: None,
            applied: Effective::default(),
            schedule_label: None,
            schedule_paused: HashSet::new(),
            dirty: HashSet::new(),
            last_flush: Instant::now(),
            visible: true,
            bridge: None,
            bridge_tx,
            bridge_rx,
            pending_pairs: HashMap::new(),
            next_pair: 0,
        })
    }

    // ---- lifecycle --------------------------------------------------------

    async fn run(mut self, cmds: async_channel::Receiver<Command>) {
        self.ensure_bridge().await;
        self.publish().await;
        let mut backoff = Duration::from_secs(1);
        loop {
            self.engine = EngineStatus::Starting;
            self.publish().await;
            let cfg = SidecarConfig {
                binary: None,
                state_dir: self.paths.engine_dir(),
                download_dir: self.settings.download_dir.clone(),
                options: self
                    .settings
                    .aria2_global_options()
                    .into_iter()
                    .filter(|(k, _)| k != "dir")
                    .collect(),
            };
            match Sidecar::start(&cfg).await {
                Ok((sidecar, client)) => {
                    backoff = Duration::from_secs(1);
                    let version = client
                        .get_version()
                        .await
                        .map(|v| v.version)
                        .unwrap_or_default();
                    tracing::info!(%version, "aria2 ready");
                    self.engine = EngineStatus::Running { version };
                    self.client = Some(client);
                    self.applied = Effective::default();
                    self.schedule_paused.clear();
                    let flow = self.connected(sidecar, &cmds).await;
                    self.client = None;
                    match flow {
                        Flow::Shutdown => break,
                        Flow::Restart => {}
                    }
                }
                Err(e) => {
                    tracing::error!("aria2 failed to start: {e:#}");
                    self.engine = EngineStatus::Failed(format!("{e:#}"));
                    self.publish().await;
                    // Keep serving commands (settings, shutdown) while waiting to retry.
                    let deadline = tokio::time::sleep(backoff);
                    tokio::pin!(deadline);
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                    loop {
                        tokio::select! {
                            _ = &mut deadline => break,
                            cmd = cmds.recv() => match cmd {
                                Ok(Command::Shutdown) | Err(_) => { self.flush(true); return; }
                                Ok(cmd) => { self.handle(cmd).await; }
                            },
                        }
                    }
                }
            }
        }
        self.flush(true);
    }

    async fn connected(
        &mut self,
        mut sidecar: Sidecar,
        cmds: &async_channel::Receiver<Command>,
    ) -> Flow {
        let client = self.client.clone().expect("client");
        let mut notes = client.subscribe();
        self.apply_schedule(true).await;
        self.sync().await;

        let mut tick = tokio::time::interval(Duration::from_millis(500));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut sched = tokio::time::interval(Duration::from_secs(15));
        let mut hidden_skip = 0u8;

        loop {
            tokio::select! {
                cmd = cmds.recv() => {
                    let Ok(cmd) = cmd else { return self.shutdown(&mut sidecar).await };
                    if matches!(cmd, Command::Shutdown) {
                        return self.shutdown(&mut sidecar).await;
                    }
                    self.handle(cmd).await;
                    self.sync().await;
                }
                note = notes.recv() => {
                    if let Ok(n) = note
                        && matches!(n.kind, NotificationKind::Complete | NotificationKind::Error | NotificationKind::Stop | NotificationKind::BtComplete)
                    {
                        self.sync().await;
                    }
                }
                _ = tick.tick() => {
                    // Poll at 500ms while visible, ~2s while hidden (tray only).
                    hidden_skip = hidden_skip.wrapping_add(1);
                    if self.visible || hidden_skip % 4 == 0 {
                        self.sync().await;
                    }
                    if client.is_closed() {
                        tracing::warn!("aria2 connection closed; restarting");
                        return Flow::Restart;
                    }
                }
                _ = sched.tick() => self.apply_schedule(false).await,
                msg = self.bridge_rx.recv() => match msg {
                    Ok(BridgeMessage::Download(req)) => self.browser_request(req).await,
                    Ok(BridgeMessage::Pair { browser, reply }) => {
                        self.next_pair += 1;
                        let id = self.next_pair;
                        // Drop requests whose extension already gave up waiting.
                        self.pending_pairs.retain(|_, r| !r.is_closed());
                        self.pending_pairs.insert(id, reply);
                        self.emit(Event::PairRequest { id, browser }).await;
                    }
                    Err(_) => {}
                },
                status = sidecar.wait() => {
                    tracing::error!("aria2 exited: {status:?}");
                    self.engine = EngineStatus::Failed("Engine stopped unexpectedly, restarting…".into());
                    self.publish().await;
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    return Flow::Restart;
                }
            }
        }
    }

    async fn shutdown(&mut self, sidecar: &mut Sidecar) -> Flow {
        if let Some(c) = &self.client {
            let _ = c.save_session().await;
            let _ = tokio::time::timeout(Duration::from_secs(2), c.shutdown()).await;
        }
        let _ = tokio::time::timeout(Duration::from_secs(3), sidecar.wait()).await;
        sidecar.kill().await;
        self.flush(true);
        Flow::Shutdown
    }

    async fn emit(&self, e: Event) {
        let _ = self.ev.send(e).await;
    }

    async fn toast(&self, kind: ToastKind, message: impl Into<String>) {
        self.emit(Event::Toast {
            kind,
            message: message.into(),
        })
        .await;
    }

    async fn publish(&self) {
        let snap = Snapshot {
            downloads: self.downloads.clone(),
            global: self.global.clone(),
            engine: self.engine.clone(),
            schedule: self.schedule_label.clone(),
        };
        self.emit(Event::Snapshot(Arc::new(snap))).await;
    }

    fn flush(&mut self, force: bool) {
        if self.dirty.is_empty() || (!force && self.last_flush.elapsed() < Duration::from_secs(5)) {
            return;
        }
        let dirty = std::mem::take(&mut self.dirty);
        let items: Vec<&Download> = self
            .downloads
            .iter()
            .filter(|d| dirty.contains(&d.id))
            .collect();
        if let Err(e) = self.store.upsert_many(items) {
            tracing::error!("store flush failed: {e:#}");
        }
        self.last_flush = Instant::now();
    }

    fn idx(&self, id: &str) -> Option<usize> {
        self.downloads.iter().position(|d| d.id == id)
    }

    fn get(&self, id: &str) -> Option<&Download> {
        self.downloads.iter().find(|d| d.id == id)
    }

    fn bind_gid(&mut self, idx: usize, gid: String) {
        if let Some(old) = self.downloads[idx].gid.take() {
            self.by_gid.remove(&old);
        }
        self.by_gid
            .insert(gid.clone(), self.downloads[idx].id.clone());
        self.downloads[idx].gid = Some(gid);
    }

    // ---- sync -------------------------------------------------------------

    async fn sync(&mut self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let keys = json!(LIST_KEYS);
        let res = client
            .multicall(vec![
                ("aria2.tellActive", vec![keys.clone()]),
                (
                    "aria2.tellWaiting",
                    vec![json!(0), json!(10_000), keys.clone()],
                ),
                ("aria2.tellStopped", vec![json!(0), json!(10_000), keys]),
                ("aria2.getGlobalStat", vec![]),
            ])
            .await;
        let Ok(mut res) = res else { return };
        if res.len() != 4 {
            return;
        }
        let parse = |v: Result<Value, yadm_aria2::Error>| -> Vec<Status> {
            v.ok()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default()
        };
        if let Some(Ok(g)) = res.pop()
            && let Ok(g) = serde_json::from_value::<yadm_aria2::GlobalStat>(g)
        {
            self.global = GlobalStats {
                dl_speed: g.download_speed,
                ul_speed: g.upload_speed,
                active: g.num_active,
                waiting: g.num_waiting,
            };
        }
        let stopped = parse(res.pop().unwrap());
        let waiting = parse(res.pop().unwrap());
        let active = parse(res.pop().unwrap());

        let mut seen = HashSet::new();
        for st in &active {
            self.merge(st, None, &mut seen).await;
        }
        for (i, st) in waiting.iter().enumerate() {
            self.merge(st, Some(i), &mut seen).await;
        }
        for st in &stopped {
            self.merge(st, None, &mut seen).await;
        }

        // Live records aria2 no longer knows about were lost (e.g. session not saved).
        for d in &mut self.downloads {
            if d.state.is_live() && !seen.contains(&d.id) {
                d.state = State::Failed;
                d.dl_speed = 0;
                d.ul_speed = 0;
                d.error = Some("The engine lost this download. Retry to restart it.".into());
                self.dirty.insert(d.id.clone());
            }
            if !matches!(d.state, State::Queued) {
                d.queue_pos = None;
            }
        }
        // Removed via another client: drop it.
        let removed: Vec<_> = self
            .downloads
            .iter()
            .filter(|d| d.state == State::Removed)
            .map(|d| d.id.clone())
            .collect();
        for id in removed {
            self.forget(&id);
        }
        self.flush(false);
        self.publish().await;
    }

    async fn merge(
        &mut self,
        st: &Status,
        queue_pos: Option<usize>,
        seen: &mut HashSet<DownloadId>,
    ) {
        // A metadata/.torrent download that spawned a follow-up: hand the record over.
        if let Some(next) = st.followed_by.first() {
            if let Some(id) = self.by_gid.get(&st.gid).cloned()
                && let Some(i) = self.idx(&id)
            {
                self.bind_gid(i, next.clone());
                seen.insert(id);
            }
            return;
        }

        let idx = match self.by_gid.get(&st.gid).and_then(|id| self.idx(id)) {
            Some(i) => i,
            None => {
                let rebind = st
                    .following
                    .as_ref()
                    .and_then(|f| self.by_gid.get(f))
                    .and_then(|id| self.idx(id))
                    .or_else(|| {
                        // A restored session entry whose gid changed: match by hash or URI.
                        let uri = st
                            .files
                            .first()
                            .and_then(|f| f.uris.first())
                            .map(|u| u.uri.as_str());
                        self.downloads.iter().position(|d| {
                            d.state.is_live()
                                && !seen.contains(&d.id)
                                && d.gid
                                    .as_ref()
                                    .is_none_or(|g| !self.by_gid.contains_key(g) || g == &st.gid)
                                && ((st.info_hash.is_some() && d.info_hash == st.info_hash)
                                    || (uri.is_some() && d.uris.first().map(String::as_str) == uri))
                        })
                    });
                match rebind {
                    Some(i) => {
                        self.bind_gid(i, st.gid.clone());
                        i
                    }
                    None => {
                        if st.status == DownloadStatus::Removed {
                            return;
                        }
                        // Unknown download (added before the DB existed or by another client).
                        let uris: Vec<String> = st
                            .files
                            .first()
                            .map(|f| f.uris.iter().map(|u| u.uri.clone()).collect())
                            .unwrap_or_default();
                        let name = st.name().unwrap_or_else(|| {
                            uris.first().map(|u| name_from_uri(u)).unwrap_or_default()
                        });
                        let d = Download {
                            id: uuid::Uuid::new_v4().to_string(),
                            kind: if st.bittorrent.is_some() {
                                DownloadKind::Torrent
                            } else {
                                DownloadKind::Http
                            },
                            category: if st.bittorrent.is_some() {
                                Category::Torrents
                            } else {
                                Category::from_name(&name)
                            },
                            name,
                            uris,
                            dir: st.dir.clone(),
                            added_at: now_ms(),
                            ..Default::default()
                        };
                        self.downloads.insert(0, d);
                        self.bind_gid(0, st.gid.clone());
                        0
                    }
                }
            }
        };

        let prev_state = self.downloads[idx].state;
        let d = &mut self.downloads[idx];
        seen.insert(d.id.clone());
        let mut state = State::from_aria2(st.status);
        // A paused download in the waiting list reports "paused"; plain waiting is queued.
        if st.status == DownloadStatus::Waiting {
            state = State::Queued;
        }
        let metadata = st.is_metadata_only();
        // A download restored from aria2's session but not started yet hasn't
        // talked to the server: aria2 reports no path, size or progress until it
        // is resumed. Keep what we stored instead of blanking it out.
        let unresolved = st.bittorrent.is_none()
            && st.total_length == 0
            && st.files.first().is_none_or(|f| f.path.is_empty());
        if !metadata {
            if !unresolved {
                if let Some(name) = st.name() {
                    d.name = name;
                }
            } else if d.name.is_empty() || d.name.contains("://") {
                // No path yet: name it after the URL's file, never the raw URL.
                if let Some(uri) = d.uris.first().cloned().or_else(|| st.name()) {
                    d.name = name_from_uri(&uri);
                }
            }
        }
        if !metadata && !(unresolved && d.total > 0) {
            d.total = st.total_length;
            d.completed = st.completed_length;
            d.files = st
                .files
                .iter()
                .map(|f| FileEntry {
                    index: f.index,
                    path: f.path.clone(),
                    length: f.length,
                    completed: f.completed_length,
                    selected: f.selected,
                })
                .collect();
        }
        if st.bittorrent.is_some() && d.kind == DownloadKind::Http {
            d.kind = DownloadKind::Torrent;
            d.category = Category::Torrents;
        }
        d.uploaded = st.upload_length;
        d.dl_speed = if state == State::Active {
            st.download_speed
        } else {
            0
        };
        d.ul_speed = if state == State::Active {
            st.upload_speed
        } else {
            0
        };
        d.connections = st.connections;
        d.seeders = st.num_seeders;
        d.info_hash = st.info_hash.clone().or(d.info_hash.take());
        if !st.dir.is_empty() {
            d.dir = st.dir.clone();
        }
        d.queue_pos = queue_pos;
        d.error = match state {
            State::Failed => Some(
                st.error_message
                    .clone()
                    .filter(|m| !m.is_empty())
                    .unwrap_or_else(|| {
                        format!("aria2 error {}", st.error_code.clone().unwrap_or_default())
                    }),
            ),
            _ => None,
        };
        d.state = state;

        let changed = prev_state != state;
        if changed {
            if state == State::Completed {
                d.finished_at = Some(now_ms());
            }
            let (id, name, err) = (d.id.clone(), d.name.clone(), d.error.clone());
            self.dirty.insert(id.clone());
            self.flush(true);
            // Only notify on real transitions, not on first sight after launch.
            if prev_state.is_live() {
                match state {
                    State::Completed => self.emit(Event::Completed { id, name }).await,
                    State::Failed => {
                        self.emit(Event::Failed {
                            id,
                            name,
                            error: err.unwrap_or_default(),
                        })
                        .await
                    }
                    _ => {}
                }
            }
        } else if state == State::Active {
            self.dirty.insert(d.id.clone());
        }
    }

    fn forget(&mut self, id: &str) {
        if let Some(i) = self.idx(id) {
            let d = self.downloads.remove(i);
            if let Some(g) = d.gid {
                self.by_gid.remove(&g);
            }
        }
        self.dirty.remove(id);
        if let Err(e) = self.store.delete(id) {
            tracing::error!("store delete failed: {e:#}");
        }
    }

    // ---- commands ---------------------------------------------------------

    async fn handle(&mut self, cmd: Command) {
        if let Err(e) = self.handle_inner(cmd).await {
            self.toast(ToastKind::Error, format!("{e:#}")).await;
        }
    }

    fn client(&self) -> anyhow::Result<Client> {
        self.client
            .clone()
            .ok_or_else(|| anyhow::anyhow!("The download engine isn't running"))
    }

    async fn handle_inner(&mut self, cmd: Command) -> anyhow::Result<()> {
        match cmd {
            Command::Add(req) => self.add(req).await?,
            Command::Pause(ids) => {
                let c = self.client()?;
                for id in ids {
                    if let Some(d) = self
                        .get(&id)
                        .filter(|d| matches!(d.state, State::Active | State::Queued))
                        && let Some(gid) = d.gid.clone()
                    {
                        self.schedule_paused.remove(&gid);
                        c.pause(&gid).await?;
                    }
                }
            }
            Command::Resume(ids) => {
                let c = self.client()?;
                let mut retry = vec![];
                for id in ids {
                    let Some(d) = self.get(&id) else { continue };
                    match (d.state, d.gid.clone()) {
                        (State::Paused, Some(gid)) => {
                            if self.applied.paused {
                                self.schedule_paused.insert(gid);
                            } else {
                                c.unpause(&gid).await?;
                            }
                        }
                        (State::Failed, _) => retry.push(id),
                        _ => {}
                    }
                }
                if !retry.is_empty() {
                    Box::pin(self.handle_inner(Command::Retry(retry))).await?;
                }
            }
            Command::Retry(ids) => {
                for id in ids {
                    self.retry(&id).await?;
                }
            }
            Command::Remove { ids, delete_files } => {
                for id in ids {
                    self.remove(&id, delete_files).await;
                }
            }
            Command::PauseAll => {
                self.schedule_paused.clear();
                self.client()?.pause_all().await?;
            }
            Command::ResumeAll => {
                if self.applied.paused {
                    self.toast(ToastKind::Info, "Downloads are paused by your schedule")
                        .await;
                } else {
                    self.client()?.unpause_all().await?;
                }
            }
            Command::ClearFinished => {
                let ids: Vec<_> = self
                    .downloads
                    .iter()
                    .filter(|d| matches!(d.state, State::Completed | State::Removed))
                    .map(|d| (d.id.clone(), d.gid.clone()))
                    .collect();
                for (id, gid) in ids {
                    if let (Some(c), Some(g)) = (&self.client, gid) {
                        let _ = c.remove_download_result(&g).await;
                    }
                    self.forget(&id);
                }
            }
            Command::SetSpeedLimit { id, limit } => {
                let Some(i) = self.idx(&id) else {
                    return Ok(());
                };
                if let Some(gid) = self.downloads[i].gid.clone()
                    && self.downloads[i].state.is_live()
                {
                    let mut o = Options::new();
                    o.insert("max-download-limit".into(), s(limit.unwrap_or(0)));
                    self.client()?.change_option(&gid, &o).await?;
                }
                self.downloads[i].speed_limit = limit.filter(|l| *l > 0);
                self.dirty.insert(id);
                self.flush(true);
            }
            Command::SetCategory { id, category } => {
                if let Some(i) = self.idx(&id) {
                    self.downloads[i].category = category;
                    self.dirty.insert(id);
                    self.flush(true);
                }
            }
            Command::Move { id, pos, absolute } => {
                if let Some(gid) = self.get(&id).and_then(|d| d.gid.clone()) {
                    self.client()?
                        .change_position(&gid, pos, if absolute { "POS_SET" } else { "POS_CUR" })
                        .await?;
                }
            }
            Command::SelectFiles { id, indexes } => {
                if let Some(gid) = self.get(&id).and_then(|d| d.gid.clone()) {
                    let list = indexes
                        .iter()
                        .map(u64::to_string)
                        .collect::<Vec<_>>()
                        .join(",");
                    let mut o = Options::new();
                    o.insert("select-file".into(), s(list));
                    self.client()?.change_option(&gid, &o).await?;
                }
            }
            Command::RequestDetails(id) => self.details(&id).await?,
            Command::UpdateSettings(new) => self.update_settings(*new).await?,
            Command::SetVisible(v) => self.visible = v,
            Command::AnswerPair { id, allow } => {
                // The first answer wins; closing the dialog afterwards is a no-op.
                if let Some(reply) = self.pending_pairs.remove(&id) {
                    let _ = reply.send(allow);
                }
            }
            Command::Shutdown => {}
        }
        Ok(())
    }

    fn target_dir(&self, category: Category, explicit: Option<PathBuf>) -> PathBuf {
        explicit.unwrap_or_else(|| {
            let base = self.settings.download_dir.clone();
            if self.settings.category_folders {
                base.join(category.folder())
            } else {
                base
            }
        })
    }

    async fn add(&mut self, req: AddRequest) -> anyhow::Result<()> {
        let c = self.client()?;
        let mut headers = req.headers.clone();
        if let Some(r) = req.referer.as_ref().filter(|r| !r.is_empty()) {
            headers.push(format!("Referer: {r}"));
        }
        if let Some(ua) = req.user_agent.as_ref().filter(|r| !r.is_empty()) {
            headers.push(format!("User-Agent: {ua}"));
        }

        let mut jobs: Vec<(Vec<String>, DownloadKind)> = vec![];
        if req.torrent.is_some() {
            jobs.push((
                vec![],
                if req.metalink {
                    DownloadKind::Metalink
                } else {
                    DownloadKind::Torrent
                },
            ));
        }
        for uri in req.uris.iter().map(|u| u.trim()).filter(|u| !u.is_empty()) {
            let kind = if uri.starts_with("magnet:") {
                DownloadKind::Magnet
            } else {
                DownloadKind::Http
            };
            jobs.push((vec![uri.to_string()], kind));
        }
        if jobs.is_empty() {
            anyhow::bail!("Nothing to download");
        }
        let single = jobs.len() == 1;

        for (uris, kind) in jobs {
            let name = match (&req.filename, uris.first()) {
                (Some(f), _) if single && !f.is_empty() => f.clone(),
                (_, Some(u)) => name_from_uri(u),
                _ => "torrent".into(),
            };
            let category = req.category.unwrap_or_else(|| match kind {
                DownloadKind::Torrent | DownloadKind::Magnet => Category::Torrents,
                _ => Category::from_name(&name),
            });
            let dir = self.target_dir(category, req.dir.clone());
            let mut o = Options::new();
            o.insert("dir".into(), s(dir.display()));
            if single && let Some(f) = req.filename.as_ref().filter(|f| !f.is_empty()) {
                o.insert("out".into(), s(safe_filename(f)));
            }
            if !headers.is_empty() {
                o.insert("header".into(), json!(headers));
            }
            let paused = req.start_paused || self.applied.paused;
            if paused {
                o.insert("pause".into(), s("true"));
            }
            if let Some(n) = req.connections {
                o.insert("split".into(), s(n.clamp(1, 64)));
                o.insert("max-connection-per-server".into(), s(n.clamp(1, 16)));
            }
            if let Some(l) = req.speed_limit {
                o.insert("max-download-limit".into(), s(l));
            }

            let gids = match (&req.torrent, kind) {
                (Some(bytes), DownloadKind::Metalink) => c.add_metalink(bytes, &o).await?,
                (Some(bytes), DownloadKind::Torrent) => vec![c.add_torrent(bytes, &[], &o).await?],
                _ => vec![c.add_uri(&uris, &o, None).await?],
            };
            for gid in gids {
                if paused && !req.start_paused {
                    self.schedule_paused.insert(gid.clone());
                }
                let d = Download {
                    id: uuid::Uuid::new_v4().to_string(),
                    kind,
                    name: name.clone(),
                    uris: uris.clone(),
                    dir: dir.display().to_string(),
                    state: if paused { State::Paused } else { State::Queued },
                    category,
                    added_at: now_ms(),
                    headers: headers.clone(),
                    speed_limit: req.speed_limit,
                    ..Default::default()
                };
                self.store.upsert(&d)?;
                self.downloads.insert(0, d);
                self.bind_gid(0, gid);
            }
        }
        Ok(())
    }

    async fn retry(&mut self, id: &str) -> anyhow::Result<()> {
        let c = self.client()?;
        let Some(i) = self.idx(id) else { return Ok(()) };
        let d = self.downloads[i].clone();
        if d.state.is_live() {
            return Ok(());
        }
        if let Some(g) = &d.gid {
            let _ = c.remove_download_result(g).await;
        }
        let mut uris = d.uris.clone();
        if uris.is_empty()
            && let Some(h) = &d.info_hash
        {
            uris.push(format!("magnet:?xt=urn:btih:{h}&dn={}", d.name));
        }
        if uris.is_empty() {
            anyhow::bail!("Can't retry \"{}\": no source URL is known", d.name);
        }
        let mut o = Options::new();
        o.insert("dir".into(), s(&d.dir));
        if d.kind == DownloadKind::Http {
            o.insert("out".into(), s(&d.name));
        }
        if !d.headers.is_empty() {
            o.insert("header".into(), json!(d.headers));
        }
        if let Some(l) = d.speed_limit {
            o.insert("max-download-limit".into(), s(l));
        }
        let gid = c.add_uri(&uris, &o, None).await?;
        let d = &mut self.downloads[i];
        d.state = State::Queued;
        d.error = None;
        d.finished_at = None;
        self.bind_gid(i, gid);
        self.dirty.insert(id.to_string());
        self.flush(true);
        Ok(())
    }

    async fn remove(&mut self, id: &str, delete_files: bool) {
        let Some(d) = self.get(id).cloned() else {
            return;
        };
        if let (Some(c), Some(gid)) = (self.client.clone(), d.gid.clone()) {
            if d.state.is_live() {
                let _ = c.force_remove(&gid).await;
            }
            let _ = c.remove_download_result(&gid).await;
            // forceRemove is asynchronous; clear the result once aria2 has stopped it.
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let _ = c.remove_download_result(&gid).await;
            });
        }
        self.forget(id);
        if delete_files {
            let paths = removable_paths(&d);
            tokio::spawn(async move {
                // Give aria2 a moment to close file handles.
                tokio::time::sleep(Duration::from_millis(700)).await;
                let existing: Vec<_> = paths.into_iter().filter(|p| p.exists()).collect();
                if !existing.is_empty()
                    && let Err(e) = trash::delete_all(&existing)
                {
                    tracing::warn!("could not move files to trash: {e}");
                }
            });
        }
    }

    async fn details(&mut self, id: &str) -> anyhow::Result<()> {
        let Some(d) = self.get(id).cloned() else {
            return Ok(());
        };
        let mut details = Details {
            id: d.id.clone(),
            ..Default::default()
        };
        if let (Some(c), Some(gid)) = (self.client.clone(), d.gid.clone()) {
            let live = d.state == State::Active;
            if live && matches!(d.kind, DownloadKind::Torrent | DownloadKind::Magnet) {
                details.peers = c.get_peers(&gid).await.unwrap_or_default();
            }
            if live && d.kind == DownloadKind::Http {
                details.servers = c
                    .get_servers(&gid)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .flat_map(|s| s.servers)
                    .collect();
            }
            if let Ok(st) = c.tell_status(&gid, Some(&["bittorrent"])).await
                && let Some(bt) = st.bittorrent
            {
                details.trackers = bt.announce_list.into_iter().flatten().collect();
            }
            if let Ok(opts) = c.get_option(&gid).await {
                details.options = opts
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            k,
                            v.as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| v.to_string()),
                        )
                    })
                    .collect();
            }
        }
        self.emit(Event::Details(Box::new(details))).await;
        Ok(())
    }

    async fn update_settings(&mut self, new: Settings) -> anyhow::Result<()> {
        new.save(&self.paths.settings())?;
        self.settings = new;
        if let Some(c) = &self.client {
            let opts: Options = self
                .settings
                .aria2_global_options()
                .into_iter()
                .map(|(k, v)| (k, s(v)))
                .collect();
            c.change_global_option(&opts).await?;
        }
        self.ensure_bridge().await;
        self.apply_schedule(true).await;
        self.emit(Event::SettingsChanged(Box::new(self.settings.clone())))
            .await;
        Ok(())
    }

    // ---- schedule ---------------------------------------------------------

    async fn apply_schedule(&mut self, force: bool) {
        let Some(c) = self.client.clone() else { return };
        let eff = schedule::evaluate(
            &self.settings.schedule,
            self.settings.max_download_limit,
            self.settings.max_upload_limit,
            chrono::Local::now().naive_local(),
        );
        if force || eff.down != self.applied.down || eff.up != self.applied.up {
            let mut o = Options::new();
            o.insert("max-overall-download-limit".into(), s(eff.down));
            o.insert("max-overall-upload-limit".into(), s(eff.up));
            if let Err(e) = c.change_global_option(&o).await {
                tracing::warn!("failed to apply limits: {e}");
            }
        }
        if eff.paused && !self.applied.paused {
            for d in &self.downloads {
                if matches!(d.state, State::Active | State::Queued)
                    && let Some(g) = &d.gid
                    && c.pause(g).await.is_ok()
                {
                    self.schedule_paused.insert(g.clone());
                }
            }
            self.toast(
                ToastKind::Info,
                format!(
                    "Paused by schedule: {}",
                    eff.label.clone().unwrap_or_default()
                ),
            )
            .await;
        } else if !eff.paused && self.applied.paused {
            for g in std::mem::take(&mut self.schedule_paused) {
                let _ = c.unpause(&g).await;
            }
            self.toast(ToastKind::Info, "Schedule window ended; resuming downloads")
                .await;
        }
        self.schedule_label = eff.label.clone();
        self.applied = eff;
    }

    // ---- browser bridge ---------------------------------------------------

    async fn ensure_bridge(&mut self) {
        let want = self.settings.bridge.clone();
        let same = self
            .bridge
            .as_ref()
            .is_some_and(|(_, cfg)| cfg.port == want.port && cfg.token == want.token);
        if want.enabled && same {
            if let Some((_, cfg)) = &mut self.bridge {
                cfg.auto_start = want.auto_start;
            }
            return;
        }
        if let Some((b, _)) = self.bridge.take() {
            b.stop();
        }
        if !want.enabled {
            return;
        }
        match yadm_bridge::start(
            BridgeConfig {
                port: want.port,
                token: want.token.clone(),
            },
            self.bridge_tx.clone(),
        )
        .await
        {
            Ok(b) => self.bridge = Some((b, want)),
            Err(e) => {
                self.toast(
                    ToastKind::Error,
                    format!("Browser integration unavailable on port {}: {e}", want.port),
                )
                .await
            }
        }
    }

    async fn browser_request(&mut self, r: BrowserRequest) {
        let mut headers = r.headers.clone();
        if let Some(c) = r.cookies.as_ref().filter(|c| !c.is_empty()) {
            headers.push(format!("Cookie: {c}"));
        }
        let req = AddRequest {
            uris: vec![r.url.clone()],
            filename: r.filename.clone().map(|f| {
                std::path::Path::new(&f)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or(f)
            }),
            headers,
            referer: r.referrer.clone(),
            user_agent: r.user_agent.clone(),
            ..Default::default()
        };
        if self.settings.bridge.auto_start {
            let name = req
                .filename
                .clone()
                .unwrap_or_else(|| name_from_uri(&r.url));
            self.handle(Command::Add(req)).await;
            self.toast(ToastKind::Success, format!("Added from browser: {name}"))
                .await;
            self.sync().await;
        } else {
            self.emit(Event::BrowserRequest(req)).await;
        }
    }
}

/// Makes a user- or browser-supplied name valid on this OS (Windows rejects
/// `<>:"/\\|?*`, control characters, trailing dots/spaces and device names
/// like `CON`), so aria2 doesn't fail to create the file.
fn safe_filename(name: &str) -> String {
    let bad = |c: char| c.is_control() || c == '/' || (cfg!(windows) && matches!(c, '<' | '>' | ':' | '"' | '\\' | '|' | '?' | '*'));
    let mut out: String = name.chars().map(|c| if bad(c) { '_' } else { c }).collect();
    if cfg!(windows) {
        out.truncate(out.trim_end_matches(['.', ' ']).len());
        let stem = out.split('.').next().unwrap_or_default().to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
        if reserved {
            out.insert(0, '_');
        }
    }
    if out.is_empty() { "download".into() } else { out }
}

/// Files to trash for a removed download: its files, aria2 control files,
/// and the top-level torrent folder when it lives inside the download dir.
fn removable_paths(d: &Download) -> Vec<PathBuf> {
    let mut out = vec![];
    let dir = PathBuf::from(&d.dir);
    if d.files.len() > 1 {
        let root = dir.join(&d.name);
        if !d.name.is_empty() && root.starts_with(&dir) && root != dir {
            out.push(root.clone());
            out.push(PathBuf::from(format!("{}.aria2", root.display())));
            return out;
        }
    }
    for f in &d.files {
        if f.path.is_empty() || f.path.starts_with("[METADATA]") {
            continue;
        }
        let p = PathBuf::from(&f.path);
        out.push(PathBuf::from(format!("{}.aria2", p.display())));
        out.push(p);
    }
    // No file list yet (never started, or restored before aria2 resolved it):
    // the partial file sits at <dir>/<name>, next to its .aria2 control file.
    if out.is_empty()
        && !d.dir.is_empty()
        && !d.name.is_empty()
        && !d.name.contains(['/', '\\'])
        && d.name != ".."
        && d.name != "."
    {
        let p = dir.join(&d.name);
        out.push(PathBuf::from(format!("{}.aria2", p.display())));
        out.push(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames() {
        assert_eq!(safe_filename("a/b.zip"), "a_b.zip");
        assert_eq!(safe_filename(""), "download");
        if cfg!(windows) {
            assert_eq!(safe_filename("what?: \"x\".zip. "), "what__ _x_.zip");
            assert_eq!(safe_filename("con.txt"), "_con.txt");
            assert_eq!(safe_filename("COM1"), "_COM1");
            assert_eq!(safe_filename("console.txt"), "console.txt");
        } else {
            assert_eq!(safe_filename("what?.zip"), "what?.zip");
        }
    }

    #[test]
    fn names() {
        assert_eq!(
            name_from_uri("https://x.org/a/My%20File.zip?x=1"),
            "My File.zip"
        );
        assert_eq!(name_from_uri("https://x.org/"), "x.org");
        assert_eq!(
            name_from_uri("magnet:?xt=urn:btih:abcdef0123456789&dn=Ubuntu+ISO"),
            "Ubuntu ISO"
        );
        assert_eq!(
            name_from_uri("magnet:?xt=urn:btih:abcdef0123456789"),
            "Magnet abcdef012345"
        );
    }

    #[test]
    fn removable() {
        let d = Download {
            dir: "/dl".into(),
            name: "show".into(),
            files: vec![
                FileEntry {
                    path: "/dl/show/a".into(),
                    ..Default::default()
                },
                FileEntry {
                    path: "/dl/show/b".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            removable_paths(&d),
            vec![PathBuf::from("/dl/show"), PathBuf::from("/dl/show.aria2")]
        );
        let d = Download {
            dir: "/dl".into(),
            files: vec![FileEntry {
                path: "/dl/a.zip".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(
            removable_paths(&d),
            vec![PathBuf::from("/dl/a.zip.aria2"), PathBuf::from("/dl/a.zip")]
        );
        // No file list yet: fall back to <dir>/<name>, but never outside dir.
        let d = Download { dir: "/dl".into(), name: "movie.mkv".into(), ..Default::default() };
        assert_eq!(
            removable_paths(&d),
            vec![PathBuf::from("/dl/movie.mkv.aria2"), PathBuf::from("/dl/movie.mkv")]
        );
        let d = Download { dir: "/dl".into(), name: "../etc".into(), ..Default::default() };
        assert!(removable_paths(&d).is_empty());
    }
}
