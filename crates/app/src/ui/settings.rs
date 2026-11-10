//! Full-page settings, Linear style: centered column, grouped rows.

use super::kit::{kbd, section_label};
use crate::store::AppStore;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Icon, Selectable as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use yadm_core::schedule::{ScheduleAction, ScheduleRule};
use yadm_core::settings::new_token;
use yadm_core::{Settings, ThemePref, format};

/// Picks a schedule action for rule `id` (`kind` = limit | pause | window).
#[derive(Clone, PartialEq, Debug, serde::Deserialize, schemars::JsonSchema, Action)]
#[action(namespace = yadm_settings)]
pub struct SetRuleAction {
    pub id: String,
    pub kind: String,
}

struct RuleRow {
    id: String,
    name: Entity<InputState>,
    start: Entity<InputState>,
    end: Entity<InputState>,
    limit: Entity<InputState>,
}

pub struct SettingsView {
    draft: Settings,
    max_concurrent: Entity<InputState>,
    split: Entity<InputState>,
    per_server: Entity<InputState>,
    user_agent: Entity<InputState>,
    proxy: Entity<InputState>,
    max_down: Entity<InputState>,
    max_up: Entity<InputState>,
    seed_ratio: Entity<InputState>,
    port: Entity<InputState>,
    aria2_options: Entity<TextareaState>,
    rules: Vec<RuleRow>,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

fn rate_text(v: u64) -> String {
    if v == 0 { String::new() } else { format::bytes(v).replace(' ', "").replace("B", "") }
}

impl SettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let s = AppStore::global(cx).read(cx).settings().clone();
        let mut subs = vec![];
        let input = |value: String, placeholder: &str, window: &mut Window, cx: &mut Context<Self>, subs: &mut Vec<Subscription>| {
            let placeholder = placeholder.to_string();
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(value));
            subs.push(cx.subscribe_in(&state, window, |this, _, ev, window, cx| {
                if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    this.commit(window, cx);
                }
            }));
            state
        };
        let max_concurrent = input(s.max_concurrent.to_string(), "5", window, cx, &mut subs);
        let split = input(s.split.to_string(), "8", window, cx, &mut subs);
        let per_server = input(s.max_connections_per_server.to_string(), "8", window, cx, &mut subs);
        let user_agent = input(s.user_agent.clone(), "aria2 default", window, cx, &mut subs);
        let proxy = input(s.proxy.clone(), "http://user:pass@host:port", window, cx, &mut subs);
        let max_down = input(rate_text(s.max_download_limit), "Unlimited", window, cx, &mut subs);
        let max_up = input(rate_text(s.max_upload_limit), "Unlimited", window, cx, &mut subs);
        let seed_ratio = input(format!("{:.1}", s.seed_ratio), "1.0", window, cx, &mut subs);
        let port = input(s.bridge.port.to_string(), "17863", window, cx, &mut subs);
        let raw: String = s.aria2_options.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        let aria2_options = cx.new(|cx| TextareaState::new(window, cx).placeholder("key=value, one per line").default_value(raw));
        subs.push(cx.subscribe_in(&aria2_options, window, |this, _, ev, window, cx| {
            if matches!(ev, InputEvent::Blur) {
                this.commit(window, cx);
            }
        }));
        let mut this = Self {
            draft: s,
            max_concurrent,
            split,
            per_server,
            user_agent,
            proxy,
            max_down,
            max_up,
            seed_ratio,
            port,
            aria2_options,
            rules: vec![],
            error: None,
            _subs: subs,
        };
        this.sync_rules(window, cx);
        this
    }

    /// Creates input rows for schedule rules that don't have them yet.
    fn sync_rules(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rules = self.draft.schedule.clone();
        self.rules.retain(|r| rules.iter().any(|x| x.id == r.id));
        for rule in rules {
            if self.rules.iter().any(|r| r.id == rule.id) {
                continue;
            }
            let mk = |v: String, ph: &str, window: &mut Window, cx: &mut Context<Self>| {
                let ph = ph.to_string();
                let st = cx.new(|cx| InputState::new(window, cx).placeholder(ph).default_value(v));
                let sub = cx.subscribe_in(&st, window, |this, _, ev, window, cx| {
                    if matches!(ev, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        this.commit(window, cx);
                    }
                });
                (st, sub)
            };
            let limit = match rule.action {
                ScheduleAction::Limit { down, .. } => rate_text(down),
                _ => String::new(),
            };
            let (name, a) = mk(rule.name.clone(), "Rule name", window, cx);
            let (start, b) = mk(rule.start.clone(), "09:00", window, cx);
            let (end, c) = mk(rule.end.clone(), "17:00", window, cx);
            let (limit, d) = mk(limit, "e.g. 500K", window, cx);
            self._subs.extend([a, b, c, d]);
            self.rules.push(RuleRow { id: rule.id.clone(), name, start, end, limit });
        }
    }

    fn val(e: &Entity<InputState>, cx: &App) -> String {
        e.read(cx).value().trim().to_string()
    }

    /// Reads all text fields into the draft, validates and saves.
    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut s = self.draft.clone();
        let mut errors = vec![];
        let num = |e: &Entity<InputState>, label: &str, min: u32, max: u32, errors: &mut Vec<String>| -> Option<u32> {
            match Self::val(e, cx).parse::<u32>() {
                Ok(v) if (min..=max).contains(&v) => Some(v),
                _ => {
                    errors.push(format!("{label} must be between {min} and {max}"));
                    None
                }
            }
        };
        if let Some(v) = num(&self.max_concurrent, "Concurrent downloads", 1, 50, &mut errors) {
            s.max_concurrent = v;
        }
        if let Some(v) = num(&self.split, "Connections per download", 1, 64, &mut errors) {
            s.split = v;
        }
        if let Some(v) = num(&self.per_server, "Connections per server", 1, 16, &mut errors) {
            s.max_connections_per_server = v;
        }
        let rate = |e: &Entity<InputState>, label: &str, errors: &mut Vec<String>| -> Option<u64> {
            let t = Self::val(e, cx);
            if t.is_empty() {
                return Some(0);
            }
            let r = format::parse_rate(&t);
            if r.is_none() {
                errors.push(format!("{label} should look like 500K or 2M"));
            }
            r
        };
        if let Some(v) = rate(&self.max_down, "Download limit", &mut errors) {
            s.max_download_limit = v;
        }
        if let Some(v) = rate(&self.max_up, "Upload limit", &mut errors) {
            s.max_upload_limit = v;
        }
        match Self::val(&self.seed_ratio, cx).parse::<f32>() {
            Ok(v) if v >= 0.0 => s.seed_ratio = v,
            _ => errors.push("Seed ratio must be a number like 1.0".into()),
        }
        match Self::val(&self.port, cx).parse::<u16>() {
            Ok(v) if v >= 1024 => s.bridge.port = v,
            _ => errors.push("Port must be between 1024 and 65535".into()),
        }
        s.user_agent = Self::val(&self.user_agent, cx);
        s.proxy = Self::val(&self.proxy, cx);
        s.aria2_options = self
            .aria2_options
            .read(cx)
            .value()
            .lines()
            .filter_map(|l| {
                let (k, v) = l.split_once('=')?;
                let k = k.trim().trim_start_matches("--");
                (!k.is_empty() && !k.starts_with("rpc")).then(|| (k.to_string(), v.trim().to_string()))
            })
            .collect();
        for row in &self.rules {
            let Some(rule) = s.schedule.iter_mut().find(|r| r.id == row.id) else { continue };
            rule.name = Self::val(&row.name, cx);
            for (field, input) in [(&mut rule.start, &row.start), (&mut rule.end, &row.end)] {
                let t = Self::val(input, cx);
                if chrono::NaiveTime::parse_from_str(&t, "%H:%M").is_ok() {
                    *field = t;
                } else {
                    errors.push(format!("“{t}” isn't a time like 09:30"));
                }
            }
            if let ScheduleAction::Limit { down, .. } = &mut rule.action {
                *down = format::parse_rate(&Self::val(&row.limit, cx)).unwrap_or(0);
            }
        }
        self.error = errors.into_iter().next();
        self.save(s, window, cx);
    }

    fn save(&mut self, s: Settings, _window: &mut Window, cx: &mut Context<Self>) {
        self.draft = s.clone();
        crate::theme::apply(s.theme, cx);
        AppStore::global(cx).update(cx, |store, cx| store.update_settings(s, cx));
        cx.notify();
    }

    fn edit(&mut self, window: &mut Window, cx: &mut Context<Self>, f: impl FnOnce(&mut Settings)) {
        let mut s = self.draft.clone();
        f(&mut s);
        self.save(s, window, cx);
    }

    fn pick_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Use folder".into()) });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(p) = paths.into_iter().next()
            {
                this.update_in(cx, |this, window, cx| this.edit(window, cx, |s| s.download_dir = p)).ok();
            }
        })
        .detach();
    }

    // ---- rendering helpers ------------------------------------------------

    fn group(title: &str, rows: Vec<AnyElement>, cx: &App) -> Div {
        let theme = cx.theme();
        v_flex().gap_2().child(section_label(title.to_string(), cx)).child(
            v_flex()
                .rounded(px(8.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .children(rows.into_iter().enumerate().map(|(i, r)| {
                    div().when(i > 0, |this| this.border_t_1().border_color(theme.border)).child(r)
                })),
        )
    }

    fn row(title: &str, description: Option<&str>, control: impl IntoElement, cx: &App) -> AnyElement {
        let theme = cx.theme();
        h_flex()
            .min_h(px(52.))
            .px_4()
            .py_2()
            .gap_4()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().text_sm().child(title.to_string()))
                    .when_some(description, |this, d| this.child(div().text_xs().text_color(theme.muted_foreground).child(d.to_string()))),
            )
            .child(control)
            .into_any_element()
    }

    fn switch(&self, id: &'static str, checked: bool, cx: &mut Context<Self>, f: fn(&mut Settings, bool)) -> Switch {
        Switch::new(id).checked(checked).on_change(cx.listener(move |this, v: &bool, window, cx| {
            let v = *v;
            this.edit(window, cx, |s| f(s, v));
        }))
    }

    fn render_rule(&self, rule: &ScheduleRule, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(row) = self.rules.iter().find(|r| r.id == rule.id) else { return div().into_any_element() };
        let id = rule.id.clone();
        let days = ["M", "T", "W", "T", "F", "S", "S"];
        let action = rule.action;
        let kind_label = action.label();
        let menu_id = id.clone();
        v_flex()
            .px_4()
            .py_3()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Switch::new(ElementId::Name(format!("rule-on-{id}").into())).small().checked(rule.enabled).on_change(cx.listener({
                            let id = id.clone();
                            move |this, v: &bool, window, cx| {
                                let v = *v;
                                this.edit(window, cx, |s| {
                                    if let Some(r) = s.schedule.iter_mut().find(|r| r.id == id) {
                                        r.enabled = v;
                                    }
                                })
                            }
                        })),
                    )
                    .child(div().flex_1().child(Input::new(&row.name).small()))
                    .child(
                        Button::new(ElementId::Name(format!("rule-kind-{id}").into()))
                            .small()
                            .label(kind_label)
                            .dropdown_menu(move |menu, _, _| {
                                let mk = |k: &str| Box::new(SetRuleAction { id: menu_id.clone(), kind: k.into() });
                                menu.menu_with_check("Limit speed", matches!(action, ScheduleAction::Limit { .. }), mk("limit"))
                                    .menu_with_check("Pause all", action == ScheduleAction::Pause, mk("pause"))
                                    .menu_with_check("Download only in this window", action == ScheduleAction::DownloadOnly, mk("window"))
                            }),
                    )
                    .child(Button::new(ElementId::Name(format!("rule-del-{id}").into())).ghost().small().icon(Icon::new(Lucide::Trash)).on_click(
                        cx.listener({
                            let id = id.clone();
                            move |this, _, window, cx| {
                                this.edit(window, cx, |s| s.schedule.retain(|r| r.id != id));
                                this.sync_rules(window, cx);
                            }
                        }),
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(ButtonGroup::new(ElementId::Name(format!("rule-days-{id}").into())).small().outline().multiple(true).children(
                        days.iter().enumerate().map(|(i, d)| {
                            Button::new(ElementId::Name(format!("rule-day-{id}-{i}").into())).label(*d).selected(rule.days[i])
                        }),
                    ).on_click(cx.listener({
                        let id = id.clone();
                        move |this, selected: &Vec<usize>, window, cx| {
                            let selected = selected.clone();
                            this.edit(window, cx, |s| {
                                if let Some(r) = s.schedule.iter_mut().find(|r| r.id == id) {
                                    r.days = std::array::from_fn(|i| selected.contains(&i));
                                }
                            })
                        }
                    })))
                    .child(div().w(px(72.)).child(Input::new(&row.start).small()))
                    .child(div().text_color(theme.muted_foreground).child("to"))
                    .child(div().w(px(72.)).child(Input::new(&row.end).small()))
                    .when(matches!(action, ScheduleAction::Limit { .. }), |this| {
                        this.child(div().text_color(theme.muted_foreground).child("max"))
                            .child(div().w(px(100.)).child(Input::new(&row.limit).small()))
                    }),
            )
            .into_any_element()
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let s = self.draft.clone();
        let dir = s.download_dir.display().to_string().replace(&std::env::var("HOME").unwrap_or_default(), "~");

        let theme_picker = ButtonGroup::new("theme-pref")
            .small()
            .outline()
            .child(Button::new("theme-system").icon(Icon::new(Lucide::Monitor)).label("System").selected(s.theme == ThemePref::System))
            .child(Button::new("theme-light").icon(Icon::new(Lucide::Sun)).label("Light").selected(s.theme == ThemePref::Light))
            .child(Button::new("theme-dark").icon(Icon::new(Lucide::Moon)).label("Dark").selected(s.theme == ThemePref::Dark))
            .on_click(cx.listener(|this, ix: &Vec<usize>, window, cx| {
                let pref = match ix.first() {
                    Some(1) => ThemePref::Light,
                    Some(2) => ThemePref::Dark,
                    _ => ThemePref::System,
                };
                this.edit(window, cx, |s| s.theme = pref);
            }));

        let general = Self::group(
            "General",
            vec![
                Self::row(
                    "Download folder",
                    Some(&dir),
                    Button::new("pick-download-dir").small().label("Change…").on_click(cx.listener(|this, _, window, cx| this.pick_dir(window, cx))),
                    cx,
                ),
                Self::row("Sort into category folders", Some("Video, Music, Documents… inside the download folder"), self.switch("category-folders", s.category_folders, cx, |s, v| s.category_folders = v), cx),
                Self::row("Appearance", None, theme_picker, cx),
                Self::row("Notify when downloads finish", None, self.switch("notify", s.notify_on_complete, cx, |s, v| s.notify_on_complete = v), cx),
                Self::row("Show in menu bar", Some("Live speed and quick controls"), self.switch("show-tray", s.show_tray, cx, |s, v| s.show_tray = v), cx),
                Self::row("Keep running when the window closes", None, self.switch("close-to-tray", s.close_to_tray, cx, |s, v| s.close_to_tray = v), cx),
            ],
            cx,
        );

        let narrow = |e: &Entity<InputState>| div().w(px(120.)).child(Input::new(e).small());
        let wide = |e: &Entity<InputState>| div().w(px(260.)).child(Input::new(e).small());
        let connections = Self::group(
            "Connections",
            vec![
                Self::row("Concurrent downloads", Some("How many downloads run at once"), narrow(&self.max_concurrent), cx),
                Self::row("Connections per download", Some("Splits a file into this many segments"), narrow(&self.split), cx),
                Self::row("Connections per server", None, narrow(&self.per_server), cx),
                Self::row("User agent", None, wide(&self.user_agent), cx),
                Self::row("Proxy", Some("Used for every protocol"), wide(&self.proxy), cx),
            ],
            cx,
        );

        let bandwidth = Self::group(
            "Bandwidth",
            vec![
                Self::row("Download limit", Some("Empty for unlimited · 500K, 2M"), narrow(&self.max_down), cx),
                Self::row("Upload limit", Some("Applies to torrents"), narrow(&self.max_up), cx),
                Self::row("Seed ratio", Some("Stop seeding at this upload/download ratio"), narrow(&self.seed_ratio), cx),
            ],
            cx,
        );

        let rules: Vec<AnyElement> = s.schedule.iter().map(|r| self.render_rule(r, cx)).collect();
        let mut schedule_rows = rules;
        schedule_rows.push(
            h_flex()
                .px_4()
                .py_2()
                .child(Button::new("add-rule").ghost().small().icon(Icon::new(Lucide::Plus)).label("Add rule").on_click(cx.listener(|this, _, window, cx| {
                    this.edit(window, cx, |s| {
                        s.schedule.push(ScheduleRule::new("Work hours", ScheduleAction::Limit { down: 512 * 1024, up: 0 }))
                    });
                    this.sync_rules(window, cx);
                })))
                .into_any_element(),
        );
        let schedule = Self::group("Schedule", schedule_rows, cx);

        let token = s.bridge.token.clone();
        let ext_dir = crate::extension_dir();
        let browser = Self::group(
            "Browser",
            vec![
                Self::row("Catch browser downloads", Some("Requires the YADM browser extension"), self.switch("bridge", s.bridge.enabled, cx, |s, v| s.bridge.enabled = v), cx),
                Self::row("Start without asking", Some("Otherwise YADM shows the New download dialog"), self.switch("bridge-auto", s.bridge.auto_start, cx, |s, v| s.bridge.auto_start = v), cx),
                Self::row("Port", None, narrow(&self.port), cx),
                Self::row(
                    "Pairing token",
                    Some("Paste this into the extension's options"),
                    h_flex()
                        .gap_1()
                        .child(div().px_2().py_1().rounded(px(4.)).bg(theme.muted).font_family(theme.mono_font_family.clone()).text_xs().child(token.clone()))
                        .child(Button::new("copy-token").ghost().small().icon(Icon::new(Lucide::Copy)).tooltip("Copy").on_click(move |_, window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(token.clone()));
                            window.push_notification(Notification::info("Token copied"), cx);
                        }))
                        .child(Button::new("regen-token").ghost().small().icon(Icon::new(Lucide::RefreshCw)).tooltip("Generate a new token").on_click(
                            cx.listener(|this, _, window, cx| this.edit(window, cx, |s| s.bridge.token = new_token())),
                        )),
                    cx,
                ),
                Self::row(
                    "Install the extension",
                    Some("Chrome: chrome://extensions → Developer mode → Load unpacked"),
                    Button::new("reveal-ext").small().label("Show folder").on_click(move |_, _, cx| cx.reveal_path(&ext_dir)),
                    cx,
                ),
            ],
            cx,
        );

        let clipboard = Self::group(
            "Clipboard",
            vec![Self::row(
                "Offer to download copied links",
                Some("Watches the clipboard for links to files and magnets"),
                self.switch("clipboard", s.clipboard_monitor, cx, |s, v| s.clipboard_monitor = v),
                cx,
            )],
            cx,
        );

        let data_dir = yadm_core::Paths::system().data_dir;
        let log = yadm_core::Paths::system().engine_dir().join("aria2.log");
        let advanced = Self::group(
            "Advanced",
            vec![
                v_flex()
                    .px_4()
                    .py_3()
                    .gap_2()
                    .child(div().text_sm().child("aria2 options"))
                    .child(div().text_xs().text_color(theme.muted_foreground).child("Applied to the engine globally, e.g. min-split-size=4M. RPC options are ignored."))
                    .child(Textarea::new(&self.aria2_options).h(px(90.)))
                    .into_any_element(),
                Self::row(
                    "Engine files",
                    None,
                    h_flex()
                        .gap_1()
                        .child(Button::new("open-log").small().label("Engine log").on_click(move |_, _, cx| cx.open_with_system(&log)))
                        .child(Button::new("open-data").small().label("Data folder").on_click(move |_, _, cx| cx.reveal_path(&data_dir))),
                    cx,
                ),
            ],
            cx,
        );

        let _ = window;
        v_flex()
            .size_full()
            .on_action(cx.listener(|this, a: &SetRuleAction, window, cx| {
                let id = a.id.clone();
                let kind = a.kind.clone();
                this.edit(window, cx, |s| {
                    if let Some(r) = s.schedule.iter_mut().find(|r| r.id == id) {
                        r.action = match kind.as_str() {
                            "pause" => ScheduleAction::Pause,
                            "window" => ScheduleAction::DownloadOnly,
                            _ => ScheduleAction::Limit { down: 512 * 1024, up: 0 },
                        };
                    }
                });
            }))
            .child(
                h_flex()
                    .h(px(44.))
                    .flex_shrink_0()
                    .px_4()
                    .gap_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().font_weight(FontWeight::MEDIUM).child("Settings"))
                    .child(div().flex_1())
                    .child(kbd("⌘ ,", cx)),
            )
            .child(
                div().id("settings-scroll").flex_1().overflow_y_scroll().child(
                    v_flex()
                        .w_full()
                        .max_w(px(680.))
                        .mx_auto()
                        .px_6()
                        .py_8()
                        .gap_8()
                        .when_some(self.error.clone(), |this, e| {
                            this.child(div().p_2().rounded(px(6.)).bg(theme.danger.opacity(0.1)).text_sm().text_color(theme.danger).child(e))
                        })
                        .child(general)
                        .child(connections)
                        .child(bandwidth)
                        .child(schedule)
                        .child(browser)
                        .child(clipboard)
                        .child(advanced),
                ),
            )
    }
}
