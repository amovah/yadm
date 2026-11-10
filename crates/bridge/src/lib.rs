//! Localhost bridge for the browser extension.
//!
//! `GET /v1/ping` answers `{app, version}`; `POST /v1/downloads` accepts a
//! [`BrowserRequest`]. Both require `Authorization: Bearer <token>`, and
//! browser-originated requests must come from an extension origin.

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
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

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub port: u16,
    pub token: String,
}

struct AppState {
    token: String,
    tx: async_channel::Sender<BrowserRequest>,
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

fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), StatusCode> {
    if let Some(origin) = headers.get(header::ORIGIN)
        && !is_extension_origin(origin)
    {
        return Err(StatusCode::FORBIDDEN);
    }
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
        .send(req)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(StatusCode::ACCEPTED)
}

/// Starts the server on `127.0.0.1:port` inside the current tokio runtime.
pub async fn start(
    config: BridgeConfig,
    tx: async_channel::Sender<BrowserRequest>,
) -> anyhow::Result<Bridge> {
    let state = Arc::new(AppState {
        token: config.token,
        tx,
    });
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|o, _| is_extension_origin(o)))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);
    let app = Router::new()
        .route("/v1/ping", get(ping))
        .route("/v1/downloads", post(add))
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
        assert_eq!(rx.recv().await.unwrap(), body);

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
}
