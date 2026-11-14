//! Spawns and owns the bundled `aria2c` process.

use crate::client::Client;
use anyhow::{Context, Result, bail};
use rand::Rng;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::{Child, Command};

#[derive(Debug, Clone)]
pub struct SidecarConfig {
    /// Path to `aria2c`; `None` uses [`find_aria2c`].
    pub binary: Option<PathBuf>,
    /// Directory for the session file and engine log.
    pub state_dir: PathBuf,
    /// Default download directory.
    pub download_dir: PathBuf,
    /// Extra `--key=value` options (e.g. max-concurrent-downloads).
    pub options: Vec<(String, String)>,
}

pub struct Sidecar {
    child: Child,
    pub port: u16,
    pub secret: String,
    pub session_file: PathBuf,
    pub log_file: PathBuf,
}

/// Looks for aria2c: `$YADM_ARIA2C`, next to the running executable (the
/// bundled sidecar in `Contents/MacOS`, or beside `yadm.exe`), then `PATH`
/// and the usual package manager locations (GUI apps launched from Finder
/// get a minimal `PATH`; Scoop/Chocolatey/winget shims may be missing too).
pub fn find_aria2c() -> Option<PathBuf> {
    find_binary("aria2c", "YADM_ARIA2C", &[])
}

/// Looks for the executable `stem` (`.exe` is appended on Windows): `$env_var`,
/// next to the running executable, `extra` (in order), then `PATH` and the
/// usual package manager locations.
pub fn find_binary(stem: &str, env_var: &str, extra: &[PathBuf]) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(env_var).map(PathBuf::from)
        && p.is_file()
    {
        return Some(p);
    }
    let name = exe_name(stem);
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let p = dir.join(&name);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut dirs: Vec<PathBuf> = extra.to_vec();
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.extend(extra_dirs());
    dirs.into_iter().map(|d| d.join(&name)).find(|p| p.is_file())
}

/// Keeps a console child of a GUI process from flashing a console window
/// on Windows; a no-op elsewhere.
pub fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    #[cfg(not(windows))]
    let _ = cmd;
}

/// Blocking-`std` variant of [`no_window`].
pub fn no_window_std(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

fn extra_dirs() -> Vec<PathBuf> {
    if cfg!(windows) {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        [
            env("USERPROFILE").map(|h| h.join("scoop").join("shims")),
            env("ProgramData").map(|p| p.join("chocolatey").join("bin")),
            env("LOCALAPPDATA").map(|p| p.join("Microsoft").join("WinGet").join("Links")),
        ]
        .into_iter()
        .flatten()
        .collect()
    } else {
        ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from).into()
    }
}

/// `stem` plus `.exe` on Windows.
pub fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

fn free_port() -> Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

fn random_secret() -> String {
    rand::rng()
        .sample_iter(&rand::distr::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

impl Sidecar {
    pub async fn spawn(config: &SidecarConfig) -> Result<Self> {
        let binary = match &config.binary {
            Some(p) => p.clone(),
            None => find_aria2c().context("aria2c not found (set YADM_ARIA2C or bundle it)")?,
        };
        std::fs::create_dir_all(&config.state_dir)?;
        std::fs::create_dir_all(&config.download_dir)?;
        let session_file = config.state_dir.join("aria2.session");
        if !session_file.exists() {
            std::fs::write(&session_file, b"")?;
        }
        let log_file = config.state_dir.join("aria2.log");
        let port = free_port()?;
        let secret = random_secret();

        let mut args: Vec<String> = vec![
            "--enable-rpc=true".into(),
            "--rpc-listen-all=false".into(),
            "--rpc-allow-origin-all=false".into(),
            format!("--rpc-listen-port={port}"),
            format!("--rpc-secret={secret}"),
            format!("--input-file={}", session_file.display()),
            format!("--save-session={}", session_file.display()),
            "--save-session-interval=15".into(),
            "--force-save=false".into(),
            "--continue=true".into(),
            "--auto-file-renaming=true".into(),
            "--allow-overwrite=false".into(),
            "--max-download-result=1000".into(),
            "--keep-unfinished-download-result=true".into(),
            "--bt-save-metadata=true".into(),
            "--follow-torrent=true".into(),
            "--summary-interval=0".into(),
            "--console-log-level=warn".into(),
            format!("--log={}", log_file.display()),
            "--log-level=notice".into(),
            format!("--dir={}", config.download_dir.display()),
            format!("--stop-with-process={}", std::process::id()),
        ];
        for (k, v) in &config.options {
            args.push(format!("--{k}={v}"));
        }

        tracing::info!(binary = %binary.display(), port, "spawning aria2c");
        let mut cmd = Command::new(&binary);
        // aria2c is a console program: without this a GUI parent on Windows
        // flashes up a console window for it.
        no_window(&mut cmd);
        let child = cmd
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to spawn {}", binary.display()))?;

        Ok(Self {
            child,
            port,
            secret,
            session_file,
            log_file,
        })
    }

    pub fn url(&self) -> String {
        format!("ws://127.0.0.1:{}/jsonrpc", self.port)
    }

    /// Spawns and connects, failing with aria2's stderr if it exits early.
    pub async fn start(config: &SidecarConfig) -> Result<(Self, Client)> {
        let mut sidecar = Self::spawn(config).await?;
        let url = sidecar.url();
        let secret = Some(sidecar.secret.clone());
        let connect = Client::connect_with_retry(&url, secret, 40);
        tokio::pin!(connect);
        tokio::select! {
            res = &mut connect => Ok((sidecar, res.context("could not connect to aria2 rpc")?)),
            status = sidecar.child.wait() => {
                let mut err = String::new();
                if let Some(mut stderr) = sidecar.child.stderr.take() {
                    use tokio::io::AsyncReadExt;
                    let _ = stderr.read_to_string(&mut err).await;
                }
                bail!("aria2c exited early ({:?}): {}", status.ok(), err.trim())
            }
        }
    }

    /// Resolves when the process exits.
    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }

    pub fn session_path(&self) -> &Path {
        &self.session_file
    }

    pub async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }
}
