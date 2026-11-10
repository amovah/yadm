# YADM

A native, keyboard-first download manager for macOS, built in Rust on
[GPUI](https://www.gpui.rs) + [GPUI Kit](https://gpui-kit.com) (shadcn-based components)
with [aria2](https://aria2.github.io) as the download engine. Its UI follows Linear: an inset content panel,
grouped dense lists, a detail sidebar, a ⌘K palette and a quiet indigo theme.

## Layout

```
crates/aria2    async JSON-RPC (WebSocket) client + aria2c sidecar process manager
crates/core     domain model, SQLite history, sync loop, scheduler, settings, engine actor
crates/bridge   localhost HTTP endpoint for the browser extension (token + Origin checked)
crates/app      the GPUI app: views, Linear theme, menu bar item, clipboard monitor
themes/         linear.json — Linear Dark / Linear Light for gpui-component
extension/      MV3 extension (Chrome, Edge, Firefox) that hands downloads to YADM
scripts/        build-aria2.sh (self-contained aria2c), bundle-macos.sh (YADM.app)
```

The UI never touches tokio: `yadm-core` runs an engine actor on its own runtime thread.
The app sends `Command`s and receives `Event`s (snapshots, toasts, details) over channels.
aria2's session file resumes partial transfers, and SQLite keeps stable ids, categories and history
that aria2 forgets after a restart.

## Develop

```sh
brew install aria2          # dev only; release builds bundle their own
cargo run -p yadm-app       # the app (binary: yadm)
cargo test --workspace      # unit + integration tests (real aria2c, skipped if missing)
```

`YADM_ARIA2C=/path/to/aria2c` overrides engine discovery. Data lives in
`~/Library/Application Support/YADM` (`settings.toml`, `yadm.db`, `engine/aria2.session`, `engine/aria2.log`).

## Package

```sh
scripts/build-aria2.sh      # → vendor/aria2c, links only system libs (AppleTLS)
scripts/bundle-macos.sh     # → dist/YADM.app (ad-hoc signed)
```

## Keyboard

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
| ⌘O / ⌘⇧O | Show in Finder / open |
| ⌥↑ ⌥↓ | Reorder queue |
| / | Filter |
| ⌘1…⌘6 | Views |
| ⌥⌘P / ⌥⌘R | Pause all / resume all |
| ⌘, | Settings |

## Browser extension

Load `extension/` unpacked (chrome://extensions → Developer mode). Then paste the pairing token from
**Settings → Browser** into the extension's options. Downloads are cancelled in the browser and sent to YADM
with cookies, referrer and user agent. If YADM isn't running, the browser keeps the download.
