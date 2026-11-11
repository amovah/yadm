# YADM

Download manager in Rust: GPUI + GPUI Kit UI (`crates/app`) on top of an aria2 engine (`crates/aria2`,
`crates/core`) with a localhost bridge for the browser extension (`crates/bridge`, `extension/`). See README.md
for features, layout and per-platform notes.

## Building on this machine (Linux)

The host lacks the `-dev` system libraries, so build and check inside the `yadm-build` Docker image:

```sh
# type-check / test
docker run --rm -u "$(id -u):$(id -g)" -e HOME=/tmp -e CARGO_HOME=/src/target/docker/cargo \
  -e CARGO_TARGET_DIR=/src/target/docker -v "$PWD":/src -w /src yadm-build \
  cargo check --profile release -p yadm-app        # or: cargo test --profile release -p yadm-core

# full build → dist/YADM-linux/yadm
scripts/build-linux.sh
```

Keep `cargo check` free of warnings.

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/) for every commit message:

```
<type>(<optional scope>): <imperative summary, lowercase, no trailing period>

<optional body: what changed and why>
```

- Types: `feat`, `fix`, `refactor`, `perf`, `style`, `docs`, `test`, `build`, `ci`, `chore`.
- Scopes follow the area touched: `app`, `core`, `aria2`, `bridge`, `extension`, `ui`, `tray`, `scripts`.
- Breaking changes: add `!` after the type/scope (`feat(core)!: …`) and a `BREAKING CHANGE:` footer.
- Keep the summary under ~72 characters; one logical change per commit.

Examples: `feat(tray): add Linux tray icon via StatusNotifierItem`,
`fix(core): keep stored size when aria2 restores an unstarted download`.
