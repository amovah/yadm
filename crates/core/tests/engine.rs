//! End-to-end: Engine + real aria2c + sqlite. Skipped without aria2c.

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use yadm_core::*;

fn serve(size: usize) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            listener.set_nonblocking(true).unwrap();
            let l = tokio::net::TcpListener::from_std(listener).unwrap();
            loop {
                let (mut sock, _) = l.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ = sock.read(&mut buf).await;
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n");
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&vec![1u8; size]).await;
                });
            }
        });
    });
    format!("http://{addr}/files/report%20final.pdf")
}

fn settings(dir: &std::path::Path) -> Settings {
    Settings {
        download_dir: dir.join("dl"),
        category_folders: true,
        bridge: BridgeSettings { enabled: false, ..Default::default() },
        ..Default::default()
    }
}

fn recv_until<T>(h: &EngineHandle, secs: u64, mut f: impl FnMut(&Event) -> Option<T>) -> T {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!left.is_zero(), "timed out waiting for event");
        let ev = futures_lite_recv(&h.events, left).expect("event");
        if let Some(v) = f(&ev) {
            return v;
        }
    }
}

fn futures_lite_recv(rx: &async_channel::Receiver<Event>, timeout: Duration) -> Option<Event> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match rx.try_recv() {
            Ok(e) => return Some(e),
            Err(async_channel::TryRecvError::Closed) => return None,
            Err(_) if std::time::Instant::now() > deadline => return None,
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

#[test]
fn add_complete_persist_restart() {
    if yadm_aria2::find_aria2c().is_none() {
        eprintln!("aria2c not found; skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::at(tmp.path().join("data"));
    let url = serve(150_000);

    let h = Engine::start(paths.clone(), settings(tmp.path()));
    recv_until(&h, 15, |e| match e {
        Event::Snapshot(s) if matches!(s.engine, EngineStatus::Running { .. }) => Some(()),
        _ => None,
    });
    h.send(Command::Add(AddRequest { uris: vec![url.clone()], ..Default::default() }));
    let id = recv_until(&h, 20, |e| match e {
        Event::Completed { id, name } => {
            assert_eq!(name, "report final.pdf");
            Some(id.clone())
        }
        _ => None,
    });
    let snap = recv_until(&h, 5, |e| match e {
        Event::Snapshot(s) => Some(s.clone()),
        _ => None,
    });
    let d = snap.downloads.iter().find(|d| d.id == id).unwrap();
    assert_eq!(d.state, State::Completed);
    assert_eq!(d.category, Category::Documents);
    assert_eq!(d.total, 150_000);
    assert!(tmp.path().join("dl/Documents/report final.pdf").exists());
    h.shutdown_blocking(Duration::from_secs(5));
    assert!(h.events.is_closed());

    // History survives restart even though aria2 forgot the finished result.
    let h = Engine::start(paths, settings(tmp.path()));
    let snap = recv_until(&h, 15, |e| match e {
        Event::Snapshot(s) if matches!(s.engine, EngineStatus::Running { .. }) => Some(s.clone()),
        _ => None,
    });
    let d = snap.downloads.iter().find(|d| d.id == id).expect("persisted");
    assert_eq!(d.state, State::Completed);
    assert!(d.finished_at.is_some());

    // Clear finished removes it.
    h.send(Command::ClearFinished);
    recv_until(&h, 5, |e| match e {
        Event::Snapshot(s) if s.downloads.is_empty() => Some(()),
        _ => None,
    });
    h.shutdown_blocking(Duration::from_secs(5));
}

/// A video through the engine: queued, run by yt-dlp, completed with its file.
/// Talks to YouTube, so only with `YADM_VIDEO_TESTS=1`.
#[test]
fn video_download_completes() {
    if std::env::var_os("YADM_VIDEO_TESTS").is_none() {
        eprintln!("YADM_VIDEO_TESTS not set; skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::at(tmp.path().join("data"));
    let h = Engine::start(paths.clone(), settings(tmp.path()));
    let tools = recv_until(&h, 30, |e| match e {
        Event::Snapshot(s) if s.tools.ytdlp_version.is_some() => Some(s.tools.clone()),
        _ => None,
    });
    h.send(Command::Add(AddRequest {
        uris: vec!["https://www.youtube.com/watch?v=jNQXAC9IVRw".into()],
        filename: Some("Me at the zoo".into()),
        video: Some(VideoMeta {
            selector: if tools.ffmpeg.is_some() { "bv*[height<=240]+ba/b".into() } else { "b".into() },
            quality_label: "240p".into(),
            ..Default::default()
        }),
        ..Default::default()
    }));
    let id = recv_until(&h, 120, |e| match e {
        Event::Completed { id, .. } => Some(id.clone()),
        Event::Failed { error, .. } => panic!("video failed: {error}"),
        _ => None,
    });
    let d = recv_until(&h, 10, |e| match e {
        Event::Snapshot(s) => s.downloads.iter().find(|d| d.id == id && d.state == State::Completed).cloned(),
        _ => None,
    });
    assert_eq!(d.kind, DownloadKind::Video);
    assert_eq!(d.category, Category::Video);
    let path = d.local_path().expect("file path");
    assert!(path.starts_with(tmp.path().join("dl").join(Category::Video.folder())), "{path:?}");
    assert!(d.name.starts_with("Me at the zoo."), "{}", d.name);
    assert!(path.is_file());
    h.shutdown_blocking(Duration::from_secs(5));
}
