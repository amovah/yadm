//! "New download" dialog, modeled on Linear's issue composer.

use super::kit::{category_icon, chip, kbd_on, muted_text};
use crate::store::AppStore;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use super::kit::PressableExt as _;
use std::cell::Cell;
use std::path::PathBuf;
use yadm_core::{AddRequest, Category, Command, format};

gpui_kit::actions!(yadm_add, [Submit]);

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
                .title(h_flex().gap_2().child(Icon::new(Lucide::ArrowDownToLine).small()).child("New download"))
                .w(px(600.))
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
            cx.observe(&urls, |this, _, cx| {
                this.error = None;
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
            _subs: subs,
        }
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
        store.read(cx).send(Command::Add(req));
        window.close_dialog(cx);
    }
}

impl Render for AddDownload {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let links = self.links(cx);
        let magnets = links.iter().filter(|l| l.starts_with("magnet:")).count();
        let single = links.len() == 1 && self.torrent.is_none() && magnets == 0;
        let settings = AppStore::global(cx).read(cx).settings().clone();
        let dir_label = self
            .dir
            .clone()
            .unwrap_or_else(|| settings.download_dir.clone())
            .display()
            .to_string()
            .replace(&std::env::var("HOME").unwrap_or_default(), "~");
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
            .child(Textarea::new(&self.urls).h(px(110.)))
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
                                .child(div().w(px(96.)).text_sm().text_color(theme.muted_foreground).child("File name"))
                                .child(div().flex_1().child(Input::new(&self.filename).small())),
                        )
                    })
                    .child(
                        h_flex()
                            .gap_3()
                            .child(div().w(px(96.)).text_sm().text_color(theme.muted_foreground).child("Save to"))
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
                            .child(div().w(px(96.)).text_sm().text_color(theme.muted_foreground).child("Category"))
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
                            .child(div().w(px(96.)).text_sm().text_color(theme.muted_foreground).child("Speed limit"))
                            .child(div().w(px(220.)).child(Input::new(&self.limit).small())),
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
                            .child(h_flex().gap_1p5().child("Add download").child(kbd_on("⌘↵", theme.primary_foreground)))
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))).pressable("submit"),
                    ),
            )
    }
}

pub fn bind(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-enter", Submit, Some("AddDownload"))]);
}
