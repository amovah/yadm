# YADM

**Yet Another Download Manager**: a native, keyboard-first download manager for macOS, Windows and Linux.

YADM is written in Rust on [GPUI](https://www.gpui.rs) + [GPUI Kit](https://gpui-kit.com) (shadcn-based
components) and uses [aria2](https://aria2.github.io) as its download engine. The UI follows Linear: an inset
content panel, grouped dense lists, a detail sidebar, a ⌘K command palette and a quiet indigo theme in light
and dark.

![YADM in dark theme](docs/screenshots/dark.png)

<details>
<summary>Light theme</summary>

![YADM in light theme](docs/screenshots/light.png)

</details>

## Features

- **HTTP(S), FTP, BitTorrent and magnet links**: segmented downloads with several connections per file,
  resumed across restarts.
- **Video downloads** from YouTube and [thousands of other sites](https://github.com/yt-dlp/yt-dlp/blob/master/supportedsites.md)
  via [yt-dlp](https://github.com/yt-dlp/yt-dlp): paste a video link, pick a quality (up to 4K/8K, or audio as M4A/MP3)
  and it downloads like any other item: pause, resume, retry, resumed across restarts.
- **Queue control**: a configurable number of simultaneous downloads (default 1); the rest wait in the
  queue and can be reordered.
- **Organised automatically**: downloads are grouped by category (video, audio, documents, archives,
  software, images, torrents), with per-category folders (on by default).
- **Browser integration**: an MV3 extension for Chrome, Edge and Firefox hands downloads to YADM with cookies,
  referrer and user agent.
- **Clipboard monitor**: offers to download links to files, video pages and magnets you copy.
- **Scheduler**: limit speed or pause everything at certain times.
- **Bandwidth limits** and seeding ratio for torrents.
- **Menu bar / system tray icon** with live speed and quick controls.
- **Keyboard first**: almost everything has a shortcut, plus a fuzzy command palette.
- **Interface size**: follows the system (including GNOME's text scaling on Linux) or 90–150%.

## Platforms

| | macOS | Windows | Linux |
| --- | --- | --- | --- |
| App | ✓ | ✓ | ✓ |
| Bundled aria2 | `YADM.app` | next to `YADM.exe` | next to `yadm` |
| Bundled yt-dlp, Deno, ffmpeg | `YADM.app` | next to `YADM.exe` | next to `yadm` |
| Tray icon / close to tray | menu bar | system tray | system tray (StatusNotifierItem) |
| Shortcut modifier | ⌘ | Ctrl | Ctrl |
| Data folder | `~/Library/Application Support/YADM` | `%APPDATA%\YADM` | `~/.local/share/YADM` |

The data folder holds `settings.toml`, `yadm.db` (history), `engine/` (aria2 session and log) and `tools/`
(yt-dlp after **Settings → Video → Update**).

## Video downloads

Release builds bundle three helper programs next to `aria2c` (licenses in `licenses/`):

- **yt-dlp** finds and downloads the video. Sites change often; **Settings → Video → Update** fetches the latest
  yt-dlp into the data folder, which then takes precedence over the bundled copy.
- **Deno** runs YouTube's JavaScript challenges; yt-dlp needs a JavaScript runtime for YouTube.
- **ffmpeg** (a minimal LGPL build, ~7 MB) merges separate video and audio streams (YouTube above 720p) and
  converts audio to MP3. Without it only single-file qualities are offered.

Links to known video sites (YouTube, Vimeo, X, TikTok, Instagram, Twitch, SoundCloud, Reddit…) and HLS/DASH
manifests are looked up as soon as they're pasted into **New download**. Links that point to files go to aria2
as before. Any other web page shows **Fetch video** to try yt-dlp on it. **Settings → Video** sets the default
quality, how many videos download at once, cookies from a browser (for sign-in or age-restricted videos) and
custom tool locations.

## Install

Download the archive for your platform from [Releases](https://github.com/amovah/yadm/releases).

- **macOS** — unzip and move `YADM.app` to Applications. The app isn't notarized, so macOS
  will refuse to open it the first time. Either run
  `xattr -dr com.apple.quarantine /Applications/YADM.app`, or try opening it and then choose
  **System Settings → Privacy & Security → Open Anyway**.
- **Windows** — run `YADM-<version>-windows-x64-setup.exe` (adds YADM to Start and to
  **Settings → Apps** for uninstalling), or unzip the portable `YADM-<version>-windows-x64.zip`
  and run `YADM.exe`. Neither is code-signed, so SmartScreen may warn; click
  **More info → Run anyway**.
- **Linux** — `tar xzf YADM-<version>-linux-x64.tar.gz` and run `YADM-linux/yadm`. If the
  bundled `aria2c` is missing, install `aria2` from your package manager.

Video support makes the downloads larger (mostly Deno): roughly 90 MB for macOS and Windows and 100 MB for Linux.

## Build and run

### macOS

```sh
brew install aria2 yt-dlp deno ffmpeg   # dev only; release builds bundle their own
cargo run -p yadm-app
```

Package a self-contained app:

```sh
scripts/build-aria2.sh      # → vendor/aria2c, links only system libs (AppleTLS)
scripts/fetch-tools.sh      # → vendor/{yt-dlp,deno}, pinned releases, SHA-256 checked
scripts/build-ffmpeg.sh     # → vendor/{ffmpeg,ffprobe}, minimal LGPL build
scripts/bundle-macos.sh     # → dist/YADM.app (ad-hoc signed)
```

The first three build universal binaries; `ARCHS=arm64` (or `x86_64`) builds one architecture, as CI does,
which keeps Deno at half the size.

### Windows

Needs the MSVC toolchain (Visual Studio Build Tools with the C++ workload).

```powershell
winget install aria2.aria2 yt-dlp.yt-dlp DenoLand.Deno Gyan.FFmpeg  # dev only
cargo run -p yadm-app
```

Package (fetches the official `aria2c.exe`, `yt-dlp.exe`, Deno and BtbN's LGPL ffmpeg into `vendor\`, checking
their SHA-256 sums; `-Aria2c` points at another aria2c):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\bundle-windows.ps1
# → dist\YADM\ and dist\YADM-<version>-windows-x64.zip
```

Closing the window keeps YADM running in the system tray while **Keep running when the window closes** and
**Show in system tray** are on. Click the tray icon to bring it back and right-click it for the menu.

### Linux

Building natively needs the `-dev` packages for fontconfig, freetype, xkbcommon, Wayland and X11. To keep them
off your machine, build in Docker instead. The script also compiles a portable `aria2c` and a minimal `ffmpeg`
and fetches yt-dlp and Deno (cached in `vendor/linux/`), so the host needs nothing beyond the libraries every
desktop already has:

```sh
scripts/build-linux.sh      # → dist/YADM-linux/{yadm, aria2c, yt-dlp, deno, ffmpeg, ffprobe, extension/}
dist/YADM-linux/yadm
docker rmi yadm-build       # drop the build image when done
```

The tray icon uses the StatusNotifierItem D-Bus protocol, which KDE, Xfce and Cinnamon show natively and GNOME
shows with the AppIndicator extension (on by default in Ubuntu). Without a tray host, closing the window quits.
On first launch YADM registers `~/.local/share/applications/dev.yadm.app.desktop` and its icon so the desktop
shows its name and icon.

### Engine discovery

`YADM_ARIA2C=/path/to/aria2c` overrides everything. Otherwise YADM looks next to its own executable, then on
`PATH`, then in the Homebrew, Scoop, Chocolatey and winget locations.

The video tools are found the same way, with `YADM_YTDLP`, `YADM_FFMPEG` and `YADM_DENO` as overrides. A path set
in **Settings → Video** wins over everything, and an updated yt-dlp in the data folder's `tools/` wins over the
bundled one.

### Tests

```sh
cargo test --workspace      # unit + integration tests (real aria2c, skipped if missing)
YADM_VIDEO_TESTS=1 cargo test -p yadm-core   # also download a short YouTube video with yt-dlp
```

## Keyboard

Shortcuts are listed with macOS keys. On Windows and Linux ⌘ is Ctrl and ⌥ is Alt (e.g. ⌥⌘P → Ctrl+Alt+P).

| Keys | Action |
| --- | --- |
| ⌘N / ⌘V | New download / paste links |
| ⌘K | Command palette |
| j k ↑ ↓ | Move |
| x, ⌘A, shift/⌘-click | Select |
| Space / Enter | Toggle details |
| p | Pause / resume |
| r | Retry |
| ⌫ / ⌘⌫ | Remove / remove and trash files |
| ⌘O / ⌘⇧O | Show in Finder/Explorer / open |
| ⌘⇧C | Copy source URL |
| ⌥↑ ⌥↓ | Reorder queue |
| / or ⌘F | Filter |
| ⌘1…⌘6 | Views |
| ⌥⌘P / ⌥⌘R | Pause all / resume all |
| ⌘, | Settings |
| ⌘W | Hide window |

## Browser extension

Load `extension/` unpacked (chrome://extensions → Developer mode → Load unpacked; **Settings → Browser → Show
folder** opens it; in Firefox use `about:debugging` or a signed `.xpi`). The extension pairs itself: on install
(or its first download) YADM asks **Connect Firefox to YADM?**, and **Allow** hands the extension its token. The
token is also in **Settings → Browser** if you'd rather paste it into the extension's options.
Downloads are cancelled in the browser and sent to YADM. If YADM isn't running, the browser keeps the
download.

The bridge listens on `127.0.0.1` only, requires the token, and accepts browser requests only from extension
origins. Pairing (`POST /v1/pair`) returns the token only after the user allows it in YADM, one request at a
time.

## Architecture

```
crates/aria2    async JSON-RPC (WebSocket) client + aria2c sidecar process manager
crates/core     domain model, SQLite history, sync loop, scheduler, settings, engine actor,
                video/: yt-dlp URL routing, probing, quality options and download jobs
crates/bridge   localhost HTTP endpoint for the browser extension (token + Origin checked)
crates/app      the GPUI app: views, Linear theme, tray icon, clipboard monitor, per-OS glue
themes/         linear.json: Linear Dark / Linear Light for gpui-component
extension/      MV3 extension (Chrome, Edge, Firefox)
scripts/        build-aria2.sh, build-ffmpeg.sh, fetch-tools.sh, bundle-macos.sh, bundle-windows.ps1, build-linux.sh
licenses/       licenses of the bundled programs (aria2, yt-dlp, Deno, FFmpeg)
```

The UI never touches tokio: `yadm-core` runs an engine actor on its own runtime thread. The app sends
`Command`s and receives `Event`s (snapshots, toasts, details) over channels. aria2's session file resumes
partial transfers, and SQLite keeps stable ids, categories and history that aria2 forgets after a restart. Video
downloads run one yt-dlp process each, supervised by the same actor; their progress is parsed from yt-dlp's
output, and its `.part` files make them resumable.

Per-OS differences live in `crates/app/src/platform.rs`. All UI sizes go through `ui::kit::dp()` so the
interface scales as one.

## License

MIT (see `license` in `Cargo.toml`). The bundled programs keep their own licenses: see `licenses/THIRD-PARTY.md`.
