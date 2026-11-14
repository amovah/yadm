//! "New download" dialog, modeled on Linear's issue composer.

use super::kit::{category_icon, chip, kbd_on, muted_text};
use crate::store::AppStore;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use crate::ui::kit::dp;
use super::kit::PressableExt as _;
use crate::platform::{keys, shortcut};
use std::cell::Cell;
use std::path::PathBuf;
use yadm_core::video::{self, QualityOption, Route, VideoInfo};
use yadm_core::{AddRequest, Category, Command, VideoMeta, format};

gpui_kit::actions!(yadm_add, [Submit]);

/// Quality picked in the video card (index into the probed options).
#[derive(Clone, PartialEq, Debug, serde::Deserialize, schemars::JsonSchema, Action)]
#[action(namespace = yadm_add)]
pub struct PickQuality {
    pub index: usize,
}

/// yt-dlp lookup for the single pasted link.
enum VideoState {
    None,
    Probing(String),
    Ready {
        url: String,
        info: Box<VideoInfo>,
        options: Vec<QualityOption>,
        selected: usize,
    },
    Failed {
        url: String,
        error: String,
    },
}

impl VideoState {
    fn url(&self) -> Option<&str> {
        match self {
            VideoState::None => None,
            VideoState::Probing(u) | VideoState::Ready { url: u, .. } | VideoState::Failed { url: u, .. } => Some(u),
        }
    }
}

/// Category choice in the dialog; `None` means auto-detect.
#[derive(Clone, PartialEq, Debug, serde::Deserialize, schemars::JsonSchema, Action)]
#[action(namespace = yadm_add)]
pub struct PickCategory {
    pub category: Option<String>,
}

pub struct AddDownload {
    urls: Entity<TextareaState>,
    filename: Entity<InputState>,
    limit: Entity<InputState>,
    torrent: Option<(String, Vec<u8>, bool)>,
    dir: Option<PathBuf>,
    category: Option<Category>,
    start_paused: bool,
    /// Extra context carried from a browser request (headers, referer…).
    base: AddRequest,
    error: Option<String>,
    video: VideoState,
    /// Download the probed link with yt-dlp (the user can switch back to a plain file).
    as_video: bool,
    _probe: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl AddDownload {
    pub fn open(req: Option<AddRequest>, window: &mut Window, cx: &mut App) {
        let view = cx.new(|cx| Self::new(req.unwrap_or_default(), window, cx));
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                view.read(cx).urls.read(cx).focus_handle(cx).focus(window, cx);
            }
            dialog
                .entrance(gpui_kit::component::dialog::DialogEntrance::None)
                .title(h_flex().gap_2().child(Icon::new(Lucide::ArrowDownToLine).small()).child("New download"))
                .w(dp(600.))
                .child(view.clone())
        });
    }

    fn new(req: AddRequest, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let urls_text = req.uris.join("\n");
        let urls = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Paste links — HTTP(S), FTP, magnet. One per line.")
                .default_value(urls_text)
        });
        let filename = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Auto")
                .default_value(req.filename.clone().unwrap_or_default())
        });
        let limit = cx.new(|cx| InputState::new(window, cx).placeholder("Unlimited (e.g. 500K, 2M)"));
        let subs = vec![
            cx.observe_in(&urls, window, |this, _, window, cx| {
                this.error = None;
                this.links_changed(window, cx);
                cx.notify();
            }),
            cx.subscribe_in(&filename, window, |this, _, ev, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.submit(window, cx);
                }
            }),
        ];
        Self {
            urls,
            filename,
            limit,
            torrent: None,
            dir: req.dir.clone(),
            category: req.category,
            start_paused: req.start_paused,
            base: req,
            error: None,
            video: VideoState::None,
            as_video: true,
            _probe: None,
            _subs: subs,
        }
        .with_initial_probe(window, cx)
    }

    fn with_initial_probe(mut self, window: &mut Window, cx: &mut Context<Self>) -> Self {
        self.links_changed(window, cx);
        self
    }

    /// The link a video lookup applies to: exactly one, no torrent file.
    fn single_link(&self, cx: &App) -> Option<String> {
        let links = self.links(cx);
        (links.len() == 1 && self.torrent.is_none()).then(|| links[0].clone())
    }

    /// Probes known video pages as soon as they're pasted.
    fn links_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let link = self.single_link(cx);
        if link.as_deref() == self.video.url() {
            return;
        }
        self.video = VideoState::None;
        self._probe = None;
        if let Some(link) = link
            && video::route(&link) == Route::Video
        {
            self.start_probe(link, window, cx);
        }
    }

    fn start_probe(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        let store = AppStore::global(cx);
        let tools = store.read(cx).snapshot().tools.clone();
        let settings = store.read(cx).settings().clone();
        if tools.ytdlp.is_none() {
            self.video = VideoState::Failed { url, error: "yt-dlp wasn't found, so videos can't be downloaded. Set its location in Settings → Video.".into() };
            return;
        }
        self.as_video = true;
        self.video = VideoState::Probing(url.clone());
        self._probe = Some(cx.spawn_in(window, async move |this, cx| {
            let (u, t, v) = (url.clone(), tools.clone(), settings.video.clone());
            let proxy = settings.proxy.clone();
            let res = cx.background_spawn(async move { video::probe(&t, &v, &proxy, &u) }).await;
            this.update(cx, |this, cx| {
                if this.video.url() != Some(url.as_str()) {
                    return;
                }
                this.video = match res {
                    Ok(info) => {
                        let options = video::quality_options(&info, tools.ffmpeg.is_some());
                        let selected = video::default_option(&options, &settings.video.default_quality);
                        if options.is_empty() {
                            VideoState::Failed { url, error: "No downloadable formats were found".into() }
                        } else {
                            VideoState::Ready { url, info: Box::new(info), options, selected }
                        }
                    }
                    Err(e) => VideoState::Failed { url, error: format!("{e:#}") },
                };
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn links(&self, cx: &App) -> Vec<String> {
        self.urls
            .read(cx)
            .value()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn pick_torrent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Choose".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let bytes = std::fs::read(&path);
            this.update(cx, |this, cx| {
                match bytes {
                    Ok(b) => {
                        let metalink = path.extension().is_some_and(|e| e != "torrent");
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        this.torrent = Some((name, b, metalink));
                    }
                    Err(e) => this.error = Some(format!("Couldn't read the file: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn pick_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Save here".into()) });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(p) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| {
                    this.dir = Some(p);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let uris = self.links(cx);
        let invalid: Vec<_> = uris
            .iter()
            .filter(|u| !["http://", "https://", "ftp://", "sftp://", "magnet:"].iter().any(|p| u.starts_with(p)))
            .collect();
        if let Some(bad) = invalid.first() {
            self.error = Some(format!("“{bad}” isn't a link YADM can download"));
            cx.notify();
            return;
        }
        if uris.is_empty() && self.torrent.is_none() {
            self.error = Some("Add at least one link or a .torrent file".into());
            cx.notify();
            return;
        }
        let limit_text = self.limit.read(cx).value().to_string();
        let speed_limit = if limit_text.trim().is_empty() {
            None
        } else {
            match format::parse_rate(&limit_text) {
                Some(v) => Some(v).filter(|v| *v > 0),
                None => {
                    self.error = Some("Speed limit should look like 500K or 2M".into());
                    cx.notify();
                    return;
                }
            }
        };
        let filename = self.filename.read(cx).value().trim().to_string();
        let store = AppStore::global(cx);
        let mut req = AddRequest {
            uris,
            dir: self.dir.clone(),
            category: self.category,
            filename: (!filename.is_empty()).then_some(filename),
            start_paused: self.start_paused,
            speed_limit,
            ..self.base.clone()
        };
        if let Some((_, bytes, metalink)) = &self.torrent {
            req.torrent = Some(bytes.clone());
            req.metalink = *metalink;
        }
        let probed = self.single_link(cx).is_some_and(|l| Some(l.as_str()) == self.video.url());
        if self.as_video
            && probed
            && let VideoState::Ready { info, options, selected, .. } = &self.video
            && let Some(o) = options.get(*selected)
        {
            if req.filename.is_none() && !info.title.trim().is_empty() {
                req.filename = Some(info.title.trim().to_string());
            }
            if req.category.is_none() {
                req.category = Some(if o.audio_only { Category::Audio } else { Category::Video });
            }
            req.video = Some(VideoMeta {
                selector: o.selector.clone(),
                quality_label: o.label.clone(),
                audio_format: o.audio_format.clone(),
                est_size: o.est_size,
                ..Default::default()
            });
        } else if probed && matches!(self.video, VideoState::Probing(_)) && self.as_video {
            self.error = Some("Still looking up the video…".into());
            cx.notify();
            return;
        }
        store.read(cx).send(Command::Add(req));
        window.close_dialog(cx);
    }
}

impl AddDownload {
    /// The video card under the links: lookup progress, the quality picker, or
    /// an offer to look a page up.
    fn render_video(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let link = self.single_link(cx)?;
        let has_ffmpeg = AppStore::global(cx).read(cx).snapshot().tools.ffmpeg.is_some();
        let card = v_flex()
            .gap_2()
            .px_3()
            .py_2p5()
            .rounded(dp(8.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.muted.opacity(0.4));
        let el = match &self.video {
            VideoState::None => {
                if video::route(&link) != Route::Probe {
                    return None;
                }
                h_flex()
                    .gap_2()
                    .child(muted_text("Is this a page with a video on it?", cx))
                    .child(div().flex_1())
                    .child(
                        Button::new("fetch-video")
                            .small()
                            .ghost()
                            .icon(Icon::new(Lucide::Film))
                            .label("Fetch video")
                            .on_click(cx.listener(move |this, _, window, cx| this.start_probe(link.clone(), window, cx)))
                            .pressable("fetch-video"),
                    )
                    .into_any_element()
            }
            VideoState::Probing(_) => card
                .child(
                    h_flex()
                        .gap_2()
                        .child(gpui_kit::component::spinner::Spinner::new().small())
                        .child(div().text_sm().text_color(theme.muted_foreground).child("Looking up the video…")),
                )
                .into_any_element(),
            VideoState::Failed { error, .. } => {
                let retry = link.clone();
                card.child(
                    h_flex()
                        .gap_2()
                        .items_start()
                        .child(Icon::new(Lucide::CircleAlert).small().text_color(theme.danger))
                        .child(div().flex_1().text_sm().child(error.clone()))
                        .child(
                            Button::new("retry-video")
                                .xsmall()
                                .ghost()
                                .icon(Icon::new(Lucide::RotateCw))
                                .on_click(cx.listener(move |this, _, window, cx| this.start_probe(retry.clone(), window, cx)))
                                .pressable("retry-video"),
                        ),
                )
                .child(muted_text("It will be added as a regular file download.", cx))
                .into_any_element()
            }
            VideoState::Ready { info, options, selected, .. } => {
                let meta: Vec<String> = [
                    info.uploader.clone(),
                    info.duration.map(|d| format::duration(d as u64)),
                    info.extractor_key.clone(),
                ]
                .into_iter()
                .flatten()
                .collect();
                let current = &options[*selected];
                let label = match current.est_size {
                    Some(n) => format!("{} · ~{}", current.label, format::bytes(n)),
                    None => current.label.clone(),
                };
                let items: Vec<(String, bool)> = options
                    .iter()
                    .enumerate()
                    .map(|(i, o)| {
                        let l = match o.est_size {
                            Some(n) => format!("{}  ·  ~{}", o.label, format::bytes(n)),
                            None => o.label.clone(),
                        };
                        (l, i == *selected)
                    })
                    .collect();
                let as_video = self.as_video;
                card.child(
                    h_flex()
                        .gap_2()
                        .child(Icon::new(Lucide::Film).small().text_color(theme.muted_foreground))
                        .child(div().flex_1().min_w_0().truncate().text_sm().font_weight(FontWeight::MEDIUM).child(info.title.clone())),
                )
                .when(!meta.is_empty(), |this| this.child(muted_text(meta.join(" · "), cx)))
                .child(
                    h_flex()
                        .gap_3()
                        .child(div().w(dp(96.)).text_sm().text_color(theme.muted_foreground).child("Quality"))
                        .child(
                            Button::new("pick-quality")
                                .small()
                                .disabled(!as_video)
                                .label(label)
                                .dropdown_menu(move |menu, _, _| {
                                    let mut menu = menu;
                                    for (i, (l, checked)) in items.iter().enumerate() {
                                        menu = menu.menu_with_check(l.clone(), *checked, Box::new(PickQuality { index: i }));
                                    }
                                    menu
                                })
                                .pressable("pick-quality"),
                        )
                        .child(div().flex_1())
                        .child(
                            Switch::new("as-video")
                                .small()
                                .label("Download as video")
                                .checked(as_video)
                                .on_change(cx.listener(|this, v: &bool, _, cx| {
                                    this.as_video = *v;
                                    cx.notify();
                                })),
                        ),
                )
                .when(!has_ffmpeg, |this| {
                    this.child(muted_text("ffmpeg wasn't found: only single-file qualities (usually up to 720p) and no MP3.", cx))
                })
                .into_any_element()
            }
        };
        Some(el)
    }
}

impl Render for AddDownload {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let links = self.links(cx);
        let magnets = links.iter().filter(|l| l.starts_with("magnet:")).count();
        let single = links.len() == 1 && self.torrent.is_none() && magnets == 0;
        let settings = AppStore::global(cx).read(cx).settings().clone();
        let dir_label = crate::platform::tilde_path(self.dir.as_ref().unwrap_or(&settings.download_dir));
        let category_label = self.category.map(|c| c.label()).unwrap_or("Auto");
        let summary = match (links.len(), magnets, &self.torrent) {
            (0, _, None) => None,
            (n, m, t) => {
                let mut parts = vec![];
                if n - m > 0 {
                    parts.push(format!("{} link{}", n - m, if n - m == 1 { "" } else { "s" }));
                }
                if m > 0 {
                    parts.push(format!("{m} magnet{}", if m == 1 { "" } else { "s" }));
                }
                if t.is_some() {
                    parts.push("1 file".into());
                }
                Some(parts.join(" · "))
            }
        };
        let current_category = self.category;

        v_flex()
            .gap_3()
            .key_context("AddDownload")
            .on_action(cx.listener(|this, _: &Submit, window, cx| this.submit(window, cx)))
            .on_action(cx.listener(|this, a: &PickCategory, _, cx| {
                this.category = a.category.as_deref().map(Category::parse);
                cx.notify();
            }))
            .on_action(cx.listener(|this, a: &PickQuality, _, cx| {
                if let VideoState::Ready { selected, options, .. } = &mut this.video {
                    *selected = a.index.min(options.len().saturating_sub(1));
                }
                cx.notify();
            }))
            .child(Textarea::new(&self.urls).h(dp(110.)))
            .children(self.render_video(cx))
            .child(
                h_flex()
                    .gap_2()
                    .children(summary.map(|s| chip(s, Some(Lucide::Link), cx)))
                    .when_some(self.torrent.as_ref(), |this, (name, _, _)| {
                        this.child(chip(name.clone(), Some(Lucide::Magnet), cx)).child(
                            Button::new("clear-torrent").ghost().xsmall().icon(Icon::new(Lucide::X)).on_click(cx.listener(|this, _, _, cx| {
                                this.torrent = None;
                                cx.notify();
                            })).pressable("clear-torrent"),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("pick-torrent")
                            .ghost()
                            .small()
                            .icon(Icon::new(Lucide::FileUp))
                            .label("Torrent or Metalink…")
                            .on_click(cx.listener(|this, _, window, cx| this.pick_torrent(window, cx))).pressable("pick-torrent"),
                    ),
            )
            .child(div().h_px().bg(theme.border))
            .child(
                v_flex()
                    .gap_2()
                    .when(single, |this| {
                        this.child(
                            h_flex()
                                .gap_3()
                                .child(div().w(dp(96.)).text_sm().text_color(theme.muted_foreground).child("File name"))
                                .child(div().flex_1().child(Input::new(&self.filename).small())),
                        )
                    })
                    .child(
                        h_flex()
                            .gap_3()
                            .child(div().w(dp(96.)).text_sm().text_color(theme.muted_foreground).child("Save to"))
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_2()
                                    .child(Icon::new(Lucide::Folder).small().text_color(theme.muted_foreground))
                                    .child(div().flex_1().truncate().text_sm().child(dir_label))
                                    .child(Button::new("pick-dir").small().label("Change…").on_click(cx.listener(|this, _, window, cx| this.pick_dir(window, cx))).pressable("pick-dir")),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .child(div().w(dp(96.)).text_sm().text_color(theme.muted_foreground).child("Category"))
                            .child(
                                Button::new("pick-category")
                                    .small()
                                    .icon(Icon::new(current_category.map(category_icon).unwrap_or(Lucide::Sparkles)))
                                    .label(category_label)
                                    .dropdown_menu(move |menu, _, _| {
                                        let mut menu = menu.menu_with_check("Auto", current_category.is_none(), Box::new(PickCategory { category: None }));
                                        menu = menu.separator();
                                        for c in Category::ALL {
                                            menu = menu.menu_with_check(c.label(), current_category == Some(c), Box::new(PickCategory { category: Some(c.as_str().into()) }));
                                        }
                                        menu
                                    }).pressable("pick-category"),
                            )
                            .when(settings.category_folders && self.dir.is_none(), |this| {
                                this.child(muted_text("Sorted into a category folder", cx))
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .child(div().w(dp(96.)).text_sm().text_color(theme.muted_foreground).child("Speed limit"))
                            .child(div().w(dp(220.)).child(Input::new(&self.limit).small())),
                    ),
            )
            .when_some(self.error.clone(), |this, e| {
                this.child(div().text_sm().text_color(theme.danger).child(e))
            })
            .child(
                h_flex()
                    .pt_2()
                    .gap_2()
                    .child(
                        Switch::new("start-paused")
                            .small()
                            .label("Download later")
                            .checked(self.start_paused)
                            .on_change(cx.listener(|this, v: &bool, _, cx| {
                                this.start_paused = *v;
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(Button::new("cancel").small().ghost().label("Cancel").on_click(|_, window, cx| window.close_dialog(cx)).pressable("cancel"))
                    .child(
                        Button::new("submit")
                            .small()
                            .primary()
                            .child(h_flex().gap_1p5().child("Add download").child(kbd_on(&shortcut("⌘↵"), theme.primary_foreground)))
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))).pressable("submit"),
                    ),
            )
    }
}

pub fn bind(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(keys("cmd-enter"), Submit, Some("AddDownload"))]);
}
