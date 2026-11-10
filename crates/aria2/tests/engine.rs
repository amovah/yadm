//! Integration tests against a real aria2c. Skipped when aria2c is absent.

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use yadm_aria2::*;

/// Minimal HTTP/1.1 server returning `size` bytes, throttled to ~`bps` bytes/sec.
async fn serve(size: usize, bps: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                let start = req
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .and_then(|r| r.split('-').next()?.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                let len = size - start;
                let head = if start > 0 {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {len}\r\nContent-Range: bytes {start}-{}/{size}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                        size - 1
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nAccept-Ranges: bytes\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n"
                    )
                };
                if sock.write_all(head.as_bytes()).await.is_err() || req.starts_with("head") {
                    return;
                }
                let chunk = vec![7u8; (bps / 20).max(1)];
                let mut left = len;
                while left > 0 {
                    let n = left.min(chunk.len());
                    if sock.write_all(&chunk[..n]).await.is_err() {
                        return;
                    }
                    left -= n;
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            });
        }
    });
    format!("http://{addr}/file.bin")
}

async fn start(dir: &std::path::Path) -> Option<(Sidecar, Client)> {
    if find_aria2c().is_none() {
        eprintln!("aria2c not found; skipping");
        return None;
    }
    let cfg = SidecarConfig {
        binary: None,
        state_dir: dir.join("state"),
        download_dir: dir.join("dl"),
        options: vec![
            ("max-connection-per-server".into(), "1".into()),
            ("split".into(), "1".into()),
        ],
    };
    Some(Sidecar::start(&cfg).await.expect("start aria2"))
}

async fn wait_for(client: &Client, gid: &str, want: DownloadStatus, secs: u64) -> Status {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let s = client.tell_status(gid, None).await.unwrap();
        if s.status == want {
            return s;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timeout waiting for {want:?}, got {:?}",
            s.status
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn add_progress_complete_with_notification() {
    let tmp = tempfile::tempdir().unwrap();
    let Some((_sc, client)) = start(tmp.path()).await else {
        return;
    };
    let v = client.get_version().await.unwrap();
    assert!(!v.version.is_empty());

    let mut notes = client.subscribe();
    let url = serve(200_000, 2_000_000).await;
    let gid = client.add_uri(&[url], &Options::new(), None).await.unwrap();
    let s = wait_for(&client, &gid, DownloadStatus::Complete, 20).await;
    assert_eq!(s.completed_length, 200_000);
    assert_eq!(s.name().as_deref(), Some("file.bin"));
    assert!(tmp.path().join("dl/file.bin").exists());

    let got = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let n = notes.recv().await.unwrap();
            if n.kind == NotificationKind::Complete && n.gid == gid {
                break;
            }
        }
    })
    .await;
    assert!(got.is_ok(), "complete notification not received");
}

#[tokio::test]
async fn pause_resume_remove_and_multicall() {
    let tmp = tempfile::tempdir().unwrap();
    let Some((_sc, client)) = start(tmp.path()).await else {
        return;
    };
    let url = serve(2_000_000, 200_000).await;
    let gid = client.add_uri(&[url], &Options::new(), None).await.unwrap();
    wait_for(&client, &gid, DownloadStatus::Active, 10).await;

    client.pause(&gid).await.unwrap();
    wait_for(&client, &gid, DownloadStatus::Paused, 10).await;

    let res = client
        .multicall(vec![
            ("aria2.tellActive", vec![]),
            (
                "aria2.tellWaiting",
                vec![serde_json::json!(0), serde_json::json!(100)],
            ),
            ("aria2.getGlobalStat", vec![]),
        ])
        .await
        .unwrap();
    assert_eq!(res.len(), 3);
    let waiting: Vec<Status> = serde_json::from_value(res[1].as_ref().unwrap().clone()).unwrap();
    assert!(
        waiting
            .iter()
            .any(|s| s.gid == gid && s.status == DownloadStatus::Paused)
    );

    let mut limit = Options::new();
    limit.insert("max-download-limit".into(), "50K".into());
    client.change_option(&gid, &limit).await.unwrap();

    client.unpause(&gid).await.unwrap();
    wait_for(&client, &gid, DownloadStatus::Active, 10).await;

    client.remove(&gid).await.unwrap();
    wait_for(&client, &gid, DownloadStatus::Removed, 10).await;
    client.remove_download_result(&gid).await.unwrap();
    assert!(client.tell_status(&gid, None).await.is_err());
}

#[tokio::test]
async fn session_resumes_after_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let Some((mut sc, client)) = start(tmp.path()).await else {
        return;
    };
    let url = serve(3_000_000, 300_000).await;
    let gid = client.add_uri(&[url], &Options::new(), None).await.unwrap();
    wait_for(&client, &gid, DownloadStatus::Active, 10).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    client.pause(&gid).await.unwrap();
    wait_for(&client, &gid, DownloadStatus::Paused, 10).await;
    client.save_session().await.unwrap();
    sc.kill().await;
    assert!(
        tmp.path().join("dl/file.bin.aria2").exists(),
        "control file keeps partial state"
    );

    let (_sc2, client2) = start(tmp.path()).await.unwrap();
    let waiting = client2.tell_waiting(0, 100, None).await.unwrap();
    assert_eq!(
        waiting.len(),
        1,
        "session should restore the paused download"
    );
    let s = &waiting[0];
    client2.unpause(&s.gid).await.unwrap();
    wait_for(&client2, &s.gid, DownloadStatus::Complete, 30).await;
}
