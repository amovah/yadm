//! JSON-RPC 2.0 client for aria2 over a WebSocket.
//!
//! One background task owns the socket. Requests are correlated by id through
//! a pending map; server notifications are fanned out on a broadcast channel.

use crate::types::*;
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("aria2 rpc error {code}: {message}")]
    Rpc { code: i64, message: String },
    #[error("connection to aria2 closed")]
    Closed,
    #[error("aria2 request timed out")]
    Timeout,
    #[error("websocket: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("decode: {0}")]
    Decode(#[from] serde_json::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Start,
    Pause,
    Stop,
    Complete,
    Error,
    BtComplete,
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub kind: NotificationKind,
    pub gid: Gid,
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;

struct Inner {
    secret: Option<String>,
    next_id: AtomicU64,
    pending: Pending,
    tx: mpsc::UnboundedSender<Message>,
    notifications: broadcast::Sender<Notification>,
    closed: Arc<AtomicBool>,
    timeout: Duration,
}

/// Cheap to clone; all clones share the same connection.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl Client {
    /// Connects to `ws://host:port/jsonrpc`.
    pub async fn connect(url: &str, secret: Option<String>) -> Result<Self> {
        let (ws, _) = tokio_tungstenite::connect_async(url).await?;
        let (mut sink, mut stream) = ws.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        let pending: Pending = Default::default();
        let (notifications, _) = broadcast::channel(256);
        let closed = Arc::new(AtomicBool::new(false));

        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if sink.send(msg).await.is_err() {
                    break;
                }
            }
        });

        {
            let pending = pending.clone();
            let notifications = notifications.clone();
            let closed = closed.clone();
            tokio::spawn(async move {
                while let Some(msg) = stream.next().await {
                    let text = match msg {
                        Ok(Message::Text(t)) => t.to_string(),
                        Ok(Message::Close(_)) | Err(_) => break,
                        Ok(_) => continue,
                    };
                    let Ok(value) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    dispatch(value, &pending, &notifications).await;
                }
                closed.store(true, Ordering::SeqCst);
                for (_, tx) in pending.lock().await.drain() {
                    let _ = tx.send(Err(Error::Closed));
                }
            });
        }

        Ok(Self {
            inner: Arc::new(Inner {
                secret,
                next_id: AtomicU64::new(1),
                pending,
                tx,
                notifications,
                closed,
                timeout: Duration::from_secs(15),
            }),
        })
    }

    /// Retries `connect` until aria2 is listening or `attempts` run out.
    pub async fn connect_with_retry(
        url: &str,
        secret: Option<String>,
        attempts: u32,
    ) -> Result<Self> {
        let mut last = Error::Closed;
        for i in 0..attempts {
            match Self::connect(url, secret.clone()).await {
                Ok(c) => return Ok(c),
                Err(e) => last = e,
            }
            tokio::time::sleep(Duration::from_millis(100 * u64::from(i + 1).min(10))).await;
        }
        Err(last)
    }

    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Notification> {
        self.inner.notifications.subscribe()
    }

    fn with_token(&self, mut params: Vec<Value>) -> Vec<Value> {
        if let Some(secret) = &self.inner.secret {
            params.insert(0, Value::String(format!("token:{secret}")));
        }
        params
    }

    /// Raw call. `params` must not include the secret token.
    pub async fn call(&self, method: &str, params: Vec<Value>) -> Result<Value> {
        if self.is_closed() {
            return Err(Error::Closed);
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let req = json!({"jsonrpc": "2.0", "id": id.to_string(), "method": method, "params": self.with_token(params)});
        let (tx, rx) = oneshot::channel();
        self.inner.pending.lock().await.insert(id, tx);
        if self
            .inner
            .tx
            .send(Message::Text(req.to_string().into()))
            .is_err()
        {
            self.inner.pending.lock().await.remove(&id);
            return Err(Error::Closed);
        }
        match tokio::time::timeout(self.inner.timeout, rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => Err(Error::Closed),
            Err(_) => {
                self.inner.pending.lock().await.remove(&id);
                Err(Error::Timeout)
            }
        }
    }

    async fn call_as<T: DeserializeOwned>(&self, method: &str, params: Vec<Value>) -> Result<T> {
        Ok(serde_json::from_value(self.call(method, params).await?)?)
    }

    /// `system.multicall`; each entry is `(method, params)` without token.
    /// Returns one result per call (rpc errors are returned per entry).
    pub async fn multicall(&self, calls: Vec<(&str, Vec<Value>)>) -> Result<Vec<Result<Value>>> {
        let methods: Vec<Value> = calls
            .into_iter()
            .map(|(m, p)| json!({"methodName": m, "params": self.with_token(p)}))
            .collect();
        // multicall itself takes no token.
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let req = json!({"jsonrpc": "2.0", "id": id.to_string(), "method": "system.multicall", "params": [methods]});
        let (tx, rx) = oneshot::channel();
        self.inner.pending.lock().await.insert(id, tx);
        self.inner
            .tx
            .send(Message::Text(req.to_string().into()))
            .map_err(|_| Error::Closed)?;
        let value = match tokio::time::timeout(self.inner.timeout, rx).await {
            Ok(Ok(res)) => res?,
            Ok(Err(_)) => return Err(Error::Closed),
            Err(_) => {
                self.inner.pending.lock().await.remove(&id);
                return Err(Error::Timeout);
            }
        };
        let Value::Array(items) = value else {
            return Ok(vec![]);
        };
        Ok(items
            .into_iter()
            .map(|item| match item {
                Value::Array(mut v) if !v.is_empty() => Ok(v.swap_remove(0)),
                Value::Object(o) => Err(rpc_error(&Value::Object(o))),
                other => Ok(other),
            })
            .collect())
    }

    // ---- typed API -----------------------------------------------------

    pub async fn add_uri(
        &self,
        uris: &[String],
        options: &Options,
        position: Option<u32>,
    ) -> Result<Gid> {
        let mut p = vec![json!(uris), json!(options)];
        if let Some(pos) = position {
            p.push(json!(pos));
        }
        self.call_as("aria2.addUri", p).await
    }

    pub async fn add_torrent(
        &self,
        torrent: &[u8],
        uris: &[String],
        options: &Options,
    ) -> Result<Gid> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(torrent);
        self.call_as(
            "aria2.addTorrent",
            vec![json!(b64), json!(uris), json!(options)],
        )
        .await
    }

    pub async fn add_metalink(&self, metalink: &[u8], options: &Options) -> Result<Vec<Gid>> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(metalink);
        self.call_as("aria2.addMetalink", vec![json!(b64), json!(options)])
            .await
    }

    pub async fn remove(&self, gid: &str) -> Result<Gid> {
        self.call_as("aria2.remove", vec![json!(gid)]).await
    }

    pub async fn force_remove(&self, gid: &str) -> Result<Gid> {
        self.call_as("aria2.forceRemove", vec![json!(gid)]).await
    }

    pub async fn pause(&self, gid: &str) -> Result<Gid> {
        self.call_as("aria2.pause", vec![json!(gid)]).await
    }

    pub async fn force_pause(&self, gid: &str) -> Result<Gid> {
        self.call_as("aria2.forcePause", vec![json!(gid)]).await
    }

    pub async fn pause_all(&self) -> Result<()> {
        self.call("aria2.pauseAll", vec![]).await.map(drop)
    }

    pub async fn unpause(&self, gid: &str) -> Result<Gid> {
        self.call_as("aria2.unpause", vec![json!(gid)]).await
    }

    pub async fn unpause_all(&self) -> Result<()> {
        self.call("aria2.unpauseAll", vec![]).await.map(drop)
    }

    pub async fn tell_status(&self, gid: &str, keys: Option<&[&str]>) -> Result<Status> {
        let mut p = vec![json!(gid)];
        if let Some(k) = keys {
            p.push(json!(k));
        }
        self.call_as("aria2.tellStatus", p).await
    }

    pub async fn tell_active(&self, keys: Option<&[&str]>) -> Result<Vec<Status>> {
        let p = keys.map(|k| vec![json!(k)]).unwrap_or_default();
        self.call_as("aria2.tellActive", p).await
    }

    pub async fn tell_waiting(
        &self,
        offset: i64,
        num: u32,
        keys: Option<&[&str]>,
    ) -> Result<Vec<Status>> {
        let mut p = vec![json!(offset), json!(num)];
        if let Some(k) = keys {
            p.push(json!(k));
        }
        self.call_as("aria2.tellWaiting", p).await
    }

    pub async fn tell_stopped(
        &self,
        offset: i64,
        num: u32,
        keys: Option<&[&str]>,
    ) -> Result<Vec<Status>> {
        let mut p = vec![json!(offset), json!(num)];
        if let Some(k) = keys {
            p.push(json!(k));
        }
        self.call_as("aria2.tellStopped", p).await
    }

    pub async fn get_files(&self, gid: &str) -> Result<Vec<File>> {
        self.call_as("aria2.getFiles", vec![json!(gid)]).await
    }

    pub async fn get_peers(&self, gid: &str) -> Result<Vec<Peer>> {
        self.call_as("aria2.getPeers", vec![json!(gid)]).await
    }

    pub async fn get_servers(&self, gid: &str) -> Result<Vec<ServerList>> {
        self.call_as("aria2.getServers", vec![json!(gid)]).await
    }

    pub async fn get_option(&self, gid: &str) -> Result<Options> {
        self.call_as("aria2.getOption", vec![json!(gid)]).await
    }

    pub async fn change_option(&self, gid: &str, options: &Options) -> Result<()> {
        self.call("aria2.changeOption", vec![json!(gid), json!(options)])
            .await
            .map(drop)
    }

    pub async fn get_global_option(&self) -> Result<Options> {
        self.call_as("aria2.getGlobalOption", vec![]).await
    }

    pub async fn change_global_option(&self, options: &Options) -> Result<()> {
        self.call("aria2.changeGlobalOption", vec![json!(options)])
            .await
            .map(drop)
    }

    pub async fn change_position(&self, gid: &str, pos: i64, how: &str) -> Result<i64> {
        self.call_as(
            "aria2.changePosition",
            vec![json!(gid), json!(pos), json!(how)],
        )
        .await
    }

    pub async fn get_global_stat(&self) -> Result<GlobalStat> {
        self.call_as("aria2.getGlobalStat", vec![]).await
    }

    pub async fn remove_download_result(&self, gid: &str) -> Result<()> {
        self.call("aria2.removeDownloadResult", vec![json!(gid)])
            .await
            .map(drop)
    }

    pub async fn purge_download_result(&self) -> Result<()> {
        self.call("aria2.purgeDownloadResult", vec![])
            .await
            .map(drop)
    }

    pub async fn save_session(&self) -> Result<()> {
        self.call("aria2.saveSession", vec![]).await.map(drop)
    }

    pub async fn get_version(&self) -> Result<Version> {
        self.call_as("aria2.getVersion", vec![]).await
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.call("aria2.shutdown", vec![]).await.map(drop)
    }
}

fn rpc_error(err: &Value) -> Error {
    Error::Rpc {
        code: err.get("code").and_then(Value::as_i64).unwrap_or(-1),
        message: err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
    }
}

async fn dispatch(
    value: Value,
    pending: &Pending,
    notifications: &broadcast::Sender<Notification>,
) {
    if let Some(method) = value.get("method").and_then(Value::as_str) {
        let kind = match method {
            "aria2.onDownloadStart" => NotificationKind::Start,
            "aria2.onDownloadPause" => NotificationKind::Pause,
            "aria2.onDownloadStop" => NotificationKind::Stop,
            "aria2.onDownloadComplete" => NotificationKind::Complete,
            "aria2.onDownloadError" => NotificationKind::Error,
            "aria2.onBtDownloadComplete" => NotificationKind::BtComplete,
            _ => return,
        };
        if let Some(events) = value.get("params").and_then(Value::as_array) {
            for ev in events {
                if let Some(gid) = ev.get("gid").and_then(Value::as_str) {
                    let _ = notifications.send(Notification {
                        kind,
                        gid: gid.to_string(),
                    });
                }
            }
        }
        return;
    }
    let Some(id) = value
        .get("id")
        .and_then(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .and_then(|s| s.parse::<u64>().ok())
    else {
        return;
    };
    let Some(tx) = pending.lock().await.remove(&id) else {
        return;
    };
    let res = match value.get("error") {
        Some(err) => Err(rpc_error(err)),
        None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
    };
    let _ = tx.send(res);
}
