//! Localhost bridge for the browser extension.
//!
//! `GET /v1/ping` answers `{app, version}`; `POST /v1/downloads` accepts a
//! [`BrowserRequest`]. Both require `Authorization: Bearer <token>`, and
//! browser-originated requests must come from an extension origin.
//!
//! `POST /v1/pair` lets an unpaired extension get the token without the user
//! copying it: the app asks the user to allow it, and only then is the token
//! returned. One pairing request is handled at a time.

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::oneshot;
use tower_http::cors::{AllowOrigin, CorsLayer};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct BrowserRequest {
    pub url: String,
    pub filename: Option<String>,
    pub referrer: Option<String>,
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
    pub file_size: Option<u64>,
    pub mime: Option<String>,
    /// Extra `Name: value` headers.
    pub headers: Vec<String>,
}

/// What the bridge hands to the app.
#[derive(Debug)]
pub enum BridgeMessage {
    Download(BrowserRequest),
    /// An extension wants the token. Answer `true` to hand it over.
    Pair {
        browser: String,
        reply: oneshot::Sender<bool>,
    },
}

/// How long a pairing request waits for the user's answer.
const PAIR_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub port: u16,
    pub token: String,
}

struct AppState {
    token: String,
    tx: async_channel::Sender<BridgeMessage>,
    /// Set while a pairing request waits for the user.
    pairing: AtomicBool,
}

/// A running bridge. Dropping it (or calling [`Bridge::stop`]) shuts the server down.
pub struct Bridge {
    pub addr: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
}

impl Bridge {
    pub fn stop(mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
    }
}

fn is_extension_origin(origin: &HeaderValue) -> bool {
    origin.to_str().is_ok_and(|o| {
        o.starts_with("chrome-extension://")
            || o.starts_with("moz-extension://")
            || o.starts_with("safari-web-extension://")
    })
}

/// Web pages always send their Origin, so anything but an extension origin is refused.
fn check_origin(headers: &HeaderMap) -> Result<(), StatusCode> {
    match headers.get(header::ORIGIN) {
        Some(origin) if !is_extension_origin(origin) => Err(StatusCode::FORBIDDEN),
        _ => Ok(()),
    }
}

fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), StatusCode> {
    check_origin(headers)?;
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if state.token.is_empty() || token != Some(state.token.as_str()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}

async fn ping(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    authorize(&state, &headers)?;
    Ok(Json(
        serde_json::json!({"app": "yadm", "version": env!("CARGO_PKG_VERSION")}),
    ))
}

async fn add(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<BrowserRequest>,
) -> Result<StatusCode, StatusCode> {
    authorize(&state, &headers)?;
    let scheme_ok = ["http://", "https://", "ftp://", "sftp://", "magnet:"]
        .iter()
        .any(|p| req.url.starts_with(p));
    if !scheme_ok {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    state
        .tx
        .send(BridgeMessage::Download(req))
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PairBody {
    /// Shown to the user, e.g. "Firefox".
    browser: String,
}

/// Clears the in-flight flag however the pairing request ends.
struct PairingSlot<'a>(&'a AtomicBool);
impl Drop for PairingSlot<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

async fn pair(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Option<Json<PairBody>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    check_origin(&headers)?;
    if state.token.is_empty() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    if state.pairing.swap(true, Ordering::SeqCst) {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    let _slot = PairingSlot(&state.pairing);
    let browser: String = body
        .map(|Json(b)| b.browser)
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(40)
        .collect();
    let browser = match browser.trim() {
        "" => "A browser".to_string(),
        b => b.to_string(),
    };
    let (reply, answer) = oneshot::channel();
    state
        .tx
        .send(BridgeMessage::Pair { browser, reply })
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    match tokio::time::timeout(PAIR_TIMEOUT, answer).await {
        Ok(Ok(true)) => Ok(Json(serde_json::json!({ "token": state.token }))),
        Ok(Ok(false)) | Ok(Err(_)) => Err(StatusCode::FORBIDDEN),
        Err(_) => Err(StatusCode::REQUEST_TIMEOUT),
    }
}

/// Starts the server on `127.0.0.1:port` inside the current tokio runtime.
pub async fn start(
    config: BridgeConfig,
    tx: async_channel::Sender<BridgeMessage>,
) -> anyhow::Result<Bridge> {
    let state = Arc::new(AppState {
        token: config.token,
        tx,
        pairing: AtomicBool::new(false),
    });
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|o, _| is_extension_origin(o)))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);
    let app = Router::new()
        .route("/v1/ping", get(ping))
        .route("/v1/downloads", post(add))
        .route("/v1/pair", post(pair))
        .layer(cors)
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", config.port)).await?;
    let addr = listener.local_addr()?;
    let (stop, stopped) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await;
    });
    tracing::info!(%addr, "browser bridge listening");
    Ok(Bridge {
        addr,
        stop: Some(stop),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn auth_origin_and_forwarding() {
        let (tx, rx) = async_channel::unbounded();
        let bridge = start(
            BridgeConfig {
                port: 0,
                token: "secret".into(),
            },
            tx,
        )
        .await
        .unwrap();
        let base = format!("http://{}", bridge.addr);
        let http = reqwest::Client::new();

        let r = http.get(format!("{base}/v1/ping")).send().await.unwrap();
        assert_eq!(r.status(), 401);
        let r = http
            .get(format!("{base}/v1/ping"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        let r = http
            .get(format!("{base}/v1/ping"))
            .bearer_auth("secret")
            .header("Origin", "https://evil.example")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403);

        let body = BrowserRequest {
            url: "https://example.com/a.zip".into(),
            ..Default::default()
        };
        let r = http
            .post(format!("{base}/v1/downloads"))
            .bearer_auth("secret")
            .header("Origin", "chrome-extension://abc")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 202);
        match rx.recv().await.unwrap() {
            BridgeMessage::Download(got) => assert_eq!(got, body),
            other => panic!("unexpected {other:?}"),
        }

        let bad = BrowserRequest {
            url: "file:///etc/passwd".into(),
            ..Default::default()
        };
        let r = http
            .post(format!("{base}/v1/downloads"))
            .bearer_auth("secret")
            .json(&bad)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 422);
        bridge.stop();
    }

    #[tokio::test]
    async fn pairing_needs_approval() {
        let (tx, rx) = async_channel::unbounded();
        let bridge = start(BridgeConfig { port: 0, token: "secret".into() }, tx).await.unwrap();
        let url = format!("http://{}/v1/pair", bridge.addr);
        let http = reqwest::Client::new();

        // Web pages can't ask.
        let r = http.post(&url).header("Origin", "https://evil.example").send().await.unwrap();
        assert_eq!(r.status(), 403);

        // Approved: the token comes back.
        let ask = tokio::spawn({
            let (http, url) = (http.clone(), url.clone());
            async move {
                http.post(&url)
                    .header("Origin", "moz-extension://abc")
                    .json(&serde_json::json!({ "browser": "Firefox" }))
                    .send()
                    .await
                    .unwrap()
            }
        });
        let BridgeMessage::Pair { browser, reply } = rx.recv().await.unwrap() else { panic!() };
        assert_eq!(browser, "Firefox");
        // A second request while the first waits is turned away.
        let r = http.post(&url).header("Origin", "moz-extension://abc").send().await.unwrap();
        assert_eq!(r.status(), 429);
        reply.send(true).unwrap();
        let r = ask.await.unwrap();
        assert_eq!(r.status(), 200);
        let body: serde_json::Value = r.json().await.unwrap();
        assert_eq!(body["token"], "secret");

        // Denied: no token.
        let ask = tokio::spawn({
            let (http, url) = (http.clone(), url.clone());
            async move { http.post(&url).header("Origin", "moz-extension://abc").send().await.unwrap() }
        });
        let BridgeMessage::Pair { reply, .. } = rx.recv().await.unwrap() else { panic!() };
        reply.send(false).unwrap();
        assert_eq!(ask.await.unwrap().status(), 403);
        bridge.stop();
    }
}
