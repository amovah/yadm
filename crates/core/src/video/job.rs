//! One yt-dlp process per video download, with progress parsed from stdout.

use super::Tools;
use super::probe::error_message;
use anyhow::{Context, Result};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use yadm_aria2::no_window;

/// Everything a job needs to start (or resume) a download.
#[derive(Debug, Clone)]
pub struct JobSpec {
    pub url: String,
    pub dir: PathBuf,
    /// Output name without extension; `None` uses the video title.
    pub filename: Option<String>,
    pub selector: String,
    pub audio_format: Option<String>,
    pub speed_limit: Option<u64>,
    /// Size estimated when probing, used until yt-dlp knows every stream.
    pub est_size: Option<u64>,
    /// Probe/common args (tool locations, cookies, proxy).
    pub common: Vec<OsString>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JobEvent {
    Progress {
        downloaded: u64,
        total: u64,
        speed: u64,
        /// A post-processing step such as "Merging", or `None` while downloading.
        note: Option<String>,
        /// The file being written, for cleanup.
        partial: Option<String>,
    },
    Finished {
        path: Option<PathBuf>,
    },
    Failed(String),
}

/// A running job; dropping it does not stop the process, call [`JobHandle::stop`].
pub struct JobHandle {
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}

impl JobHandle {
    /// Stops the process tree; no further events are sent.
    pub fn stop(mut self) {
        if let Some(c) = self.cancel.take() {
            let _ = c.send(());
        }
    }
}

const TAG: &str = "YADM|";
const PP_TAG: &str = "YADMPP|";
const FILE_TAG: &str = "YADMFILE|";

fn args(tools: &Tools, spec: &JobSpec) -> Vec<OsString> {
    let mut a = spec.common.clone();
    let template = match &spec.filename {
        // A literal name: escape yt-dlp's template syntax.
        Some(f) => format!("{}.%(ext)s", f.replace('%', "%%")),
        None => "%(title).180B [%(id)s].%(ext)s".into(),
    };
    for s in [
        "--newline",
        "--progress",
        "--no-simulate",
        "--continue",
        "--no-mtime",
        "--no-warnings",
        "--progress-template",
        "download:YADM|%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(info.format_id)s|%(progress.filename)s",
        "--progress-template",
        "postprocess:YADMPP|%(progress.status)s|%(progress.postprocessor)s",
        "--print",
        "after_move:YADMFILE|%(filepath)s",
        "-f",
    ] {
        a.push(s.into());
    }
    a.push(spec.selector.clone().into());
    if let Some(fmt) = &spec.audio_format {
        for s in ["-x", "--audio-format", fmt, "--audio-quality", "0"] {
            a.push(s.into());
        }
    } else {
        // Same resolution, but prefer H.264/AAC in MP4: plays everywhere
        // (QuickTime, older TVs); 4K+ still falls back to VP9/AV1.
        a.push("-S".into());
        a.push("res,vcodec:h264,ext:mp4:m4a".into());
        if tools.ffmpeg.is_some() {
            a.push("--merge-output-format".into());
            a.push("mp4/mkv".into());
        }
    }
    if let Some(l) = spec.speed_limit.filter(|l| *l > 0) {
        a.push("--limit-rate".into());
        a.push(l.to_string().into());
    }
    a.push("-P".into());
    a.push(spec.dir.clone().into());
    a.push("-o".into());
    a.push(template.into());
    a.push("--".into());
    a.push(spec.url.clone().into());
    a
}

/// Starts yt-dlp; events arrive on `tx` tagged with `tag` until it exits.
pub fn spawn<T: Clone + Send + 'static>(
    tools: &Tools,
    spec: &JobSpec,
    tag: T,
    tx: async_channel::Sender<(T, JobEvent)>,
) -> Result<JobHandle> {
    let ytdlp = tools.ytdlp.clone().context("yt-dlp was not found")?;
    std::fs::create_dir_all(&spec.dir)
        .with_context(|| format!("can't create {}", spec.dir.display()))?;
    let mut cmd = Command::new(&ytdlp);
    no_window(&mut cmd);
    // Own process group, so stopping also ends PyInstaller's child and ffmpeg.
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd
        .args(args(tools, spec))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to run {}", ytdlp.display()))?;
    let pid = child.id();
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

    let out_tx = tx.clone();
    let out_tag = tag.clone();
    let mut progress = Progress::new(spec.est_size);
    let reader = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut path = None;
        while let Ok(Some(line)) = lines.next_line().await {
            match progress.line(&line) {
                Some(Line::Event(e)) => {
                    let _ = out_tx.send((out_tag.clone(), e)).await;
                }
                Some(Line::File(p)) => path = Some(p),
                None => {}
            }
        }
        path
    });
    let err_reader = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut tail: Vec<String> = vec![];
        while let Ok(Some(line)) = lines.next_line().await {
            tail.push(line);
            if tail.len() > 30 {
                tail.remove(0);
            }
        }
        tail.join("\n")
    });

    tokio::spawn(async move {
        tokio::select! {
            status = child.wait() => {
                let path = reader.await.ok().flatten();
                let err = err_reader.await.unwrap_or_default();
                let ev = match status {
                    Ok(s) if s.success() => JobEvent::Finished { path },
                    Ok(_) => JobEvent::Failed(error_message(&err)),
                    Err(e) => JobEvent::Failed(e.to_string()),
                };
                let _ = tx.send((tag, ev)).await;
            }
            _ = cancel_rx => {
                if let Some(pid) = pid {
                    terminate_tree(pid);
                }
                if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
                    if let Some(pid) = pid {
                        kill_tree(pid);
                    }
                    let _ = child.kill().await;
                }
            }
        }
    });
    Ok(JobHandle {
        cancel: Some(cancel_tx),
    })
}

/// Asks the process group to stop (lets yt-dlp flush its `.part` file).
fn terminate_tree(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGTERM);
    }
    #[cfg(windows)]
    kill_tree(pid);
}

fn kill_tree(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("taskkill");
        yadm_aria2::no_window_std(&mut cmd);
        let _ = cmd
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

enum Line {
    Event(JobEvent),
    File(PathBuf),
}

/// Folds per-stream progress into one bar: a merged download fetches video
/// then audio, each reported from zero.
struct Progress {
    est_total: u64,
    /// Bytes of streams already finished.
    done: u64,
    current: Option<String>,
    cur_total: u64,
}

impl Progress {
    fn new(est: Option<u64>) -> Self {
        Self {
            est_total: est.unwrap_or(0),
            done: 0,
            current: None,
            cur_total: 0,
        }
    }

    fn line(&mut self, line: &str) -> Option<Line> {
        let num = |s: &str| s.trim().parse::<f64>().ok().map(|v| v.max(0.0) as u64);
        if let Some(rest) = line.strip_prefix(FILE_TAG) {
            return Some(Line::File(PathBuf::from(rest.trim_end())));
        }
        if let Some(rest) = line.strip_prefix(PP_TAG) {
            let mut it = rest.splitn(2, '|');
            let status = it.next().unwrap_or_default();
            let pp = it.next().unwrap_or_default().trim();
            let note = match pp {
                "Merger" => "Merging",
                "ExtractAudio" => "Converting",
                "MoveFiles" | "MoveFilesAfterDownload" => return None,
                _ => "Processing",
            };
            if status == "finished" {
                return None;
            }
            let total = self.total().max(self.done);
            return Some(Line::Event(JobEvent::Progress {
                downloaded: total,
                total,
                speed: 0,
                note: Some(note.into()),
                partial: None,
            }));
        }
        let rest = line.strip_prefix(TAG)?;
        let f: Vec<&str> = rest.splitn(7, '|').collect();
        if f.len() < 7 {
            return None;
        }
        let (status, downloaded, total, estimate, speed, fid, file) =
            (f[0], num(f[1]), num(f[2]), num(f[3]), num(f[4]), f[5], f[6].trim_end());
        if self.current.as_deref() != Some(fid) {
            if self.current.is_some() {
                self.done += self.cur_total;
            }
            self.current = Some(fid.to_string());
            self.cur_total = 0;
        }
        let cur_total = total.or(estimate).unwrap_or(0);
        let downloaded = downloaded.unwrap_or(0);
        self.cur_total = cur_total.max(downloaded);
        Some(Line::Event(JobEvent::Progress {
            downloaded: self.done + downloaded,
            total: self.total(),
            speed: if status == "downloading" { speed.unwrap_or(0) } else { 0 },
            note: None,
            partial: (!file.is_empty() && file != "NA").then(|| file.to_string()),
        }))
    }

    fn total(&self) -> u64 {
        self.est_total.max(self.done + self.cur_total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(p: &mut Progress, l: &str) -> (u64, u64, u64, Option<String>) {
        match p.line(l) {
            Some(Line::Event(JobEvent::Progress { downloaded, total, speed, note, .. })) => {
                (downloaded, total, speed, note)
            }
            _ => panic!("no progress for {l}"),
        }
    }

    #[test]
    fn folds_streams() {
        let mut p = Progress::new(Some(150));
        assert_eq!(ev(&mut p, "YADM|downloading|10|100|NA|5|137|/d/v.f137.mp4"), (10, 150, 5, None));
        assert_eq!(ev(&mut p, "YADM|finished|100|100|NA|NA|137|/d/v.f137.mp4"), (100, 150, 0, None));
        // Audio stream starts from zero; the bar keeps going.
        assert_eq!(ev(&mut p, "YADM|downloading|20|NA|60|7|251|/d/v.f251.webm"), (120, 160, 7, None));
        assert_eq!(
            ev(&mut p, "YADMPP|started|Merger"),
            (160, 160, 0, Some("Merging".into()))
        );
        assert!(p.line("YADMPP|started|MoveFiles").is_none());
        match p.line("YADMFILE|/d/My video.mp4") {
            Some(Line::File(f)) => assert_eq!(f, PathBuf::from("/d/My video.mp4")),
            _ => panic!(),
        }
        assert!(p.line("[youtube] random log").is_none());
        // Filenames may contain the separator.
        match p.line("YADM|downloading|1|2|NA|NA|18|/d/a|b.mp4") {
            Some(Line::Event(JobEvent::Progress { partial, .. })) => {
                assert_eq!(partial.as_deref(), Some("/d/a|b.mp4"))
            }
            _ => panic!(),
        }
    }

    /// Downloads a real 19 s video; only with `YADM_VIDEO_TESTS=1`.
    #[tokio::test]
    async fn downloads_youtube() {
        if std::env::var_os("YADM_VIDEO_TESTS").is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::resolve(&Default::default(), &crate::Paths::at(dir.path()));
        if tools.ytdlp.is_none() {
            return;
        }
        let spec = JobSpec {
            url: "https://www.youtube.com/watch?v=jNQXAC9IVRw".into(),
            dir: dir.path().join("dl"),
            filename: Some("zoo".into()),
            selector: if tools.ffmpeg.is_some() { "bv*[height<=240]+ba/b".into() } else { "b".into() },
            audio_format: None,
            speed_limit: None,
            est_size: None,
            common: super::super::common_args(&tools, &Default::default(), ""),
        };
        let (tx, rx) = async_channel::unbounded();
        let _job = spawn(&tools, &spec, 1u8, tx).unwrap();
        let mut progressed = false;
        loop {
            let (_, ev) = tokio::time::timeout(Duration::from_secs(120), rx.recv())
                .await
                .expect("timed out")
                .unwrap();
            match ev {
                JobEvent::Progress { downloaded, .. } => progressed |= downloaded > 0,
                JobEvent::Finished { path } => {
                    let path = path.expect("final path");
                    assert!(path.starts_with(dir.path().join("dl")), "{path:?}");
                    assert!(path.file_name().unwrap().to_string_lossy().starts_with("zoo."));
                    assert!(std::fs::metadata(&path).unwrap().len() > 0);
                    break;
                }
                JobEvent::Failed(e) => panic!("{e}"),
            }
        }
        assert!(progressed);
    }

    #[test]
    fn builds_args() {
        let tools = Tools {
            ytdlp: Some("yt-dlp".into()),
            ffmpeg: Some("/x/ffmpeg".into()),
            ..Default::default()
        };
        let spec = JobSpec {
            url: "https://youtu.be/x".into(),
            dir: "/dl".into(),
            filename: Some("100% done".into()),
            selector: "b".into(),
            audio_format: None,
            speed_limit: Some(1000),
            est_size: None,
            common: vec![],
        };
        let a: Vec<String> = args(&tools, &spec).iter().map(|s| s.to_string_lossy().into_owned()).collect();
        let pos = |s: &str| a.iter().position(|x| x == s).unwrap();
        assert_eq!(a[pos("-o") + 1], "100%% done.%(ext)s");
        assert_eq!(a[pos("--merge-output-format") + 1], "mp4/mkv");
        assert_eq!(a[pos("-S") + 1], "res,vcodec:h264,ext:mp4:m4a");
        assert_eq!(a[pos("--limit-rate") + 1], "1000");
        assert_eq!(a[a.len() - 2..], ["--".to_string(), "https://youtu.be/x".to_string()]);
    }
}
