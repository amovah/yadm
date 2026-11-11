//! Full-window settings, Linear style: its own sidebar of sections with a
//! search box, and one page per section in the inset panel.

use super::kit::{section_label, window_drag_area};
use crate::actions::{CloseOverlay, FocusSearch};
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
use crate::ui::kit::dp;
use super::kit::PressableExt as _;
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

/// Asks the owner to leave settings.
pub enum SettingsEvent {
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Appearance,
    Connections,
    Bandwidth,
    Schedule,
    Browser,
    Clipboard,
    Advanced,
}

impl Section {
    /// Sidebar groups, in order.
    const GROUPS: [(&'static str, &'static [Section]); 4] = [
        ("Preferences", &[Section::General, Section::Appearance]),
        ("Transfers", &[Section::Connections, Section::Bandwidth, Section::Schedule]),
        ("Integrations", &[Section::Browser, Section::Clipboard]),
        ("System", &[Section::Advanced]),
    ];

    fn all() -> impl Iterator<Item = Section> {
        Self::GROUPS.iter().flat_map(|(_, s)| s.iter().copied())
    }

    fn label(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Appearance => "Appearance",
            Section::Connections => "Connections",
            Section::Bandwidth => "Bandwidth",
            Section::Schedule => "Schedule",
            Section::Browser => "Browser",
            Section::Clipboard => "Clipboard",
            Section::Advanced => "Advanced",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Section::General => "Where downloads go and how YADM behaves",
            Section::Appearance => if crate::platform::IS_MAC {
                "Theme and menu bar presence"
            } else if crate::platform::HAS_TRAY {
                "Theme and system tray presence"
            } else {
                "Theme"
            },
            Section::Connections => "How many transfers run and how they connect",
            Section::Bandwidth => "Speed limits and seeding",
            Section::Schedule => "Limit or pause downloads at certain times",
            Section::Browser => "Catch downloads from your browser",
            Section::Clipboard => "Pick up links you copy",
            Section::Advanced => "Engine options and files",
        }
    }

    fn icon(self) -> Lucide {
        match self {
            Section::General => Lucide::Settings,
            Section::Appearance => Lucide::Palette,
            Section::Connections => Lucide::Network,
            Section::Bandwidth => Lucide::Gauge,
            Section::Schedule => Lucide::CalendarClock,
            Section::Browser => Lucide::Globe,
            Section::Clipboard => Lucide::Clipboard,
            Section::Advanced => Lucide::SquareTerminal,
        }
    }
}

/// One searchable row of a settings page.
struct Entry {
    /// Lowercased title, description and keywords.
    haystack: String,
    el: AnyElement,
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
    section: Section,
    search: Entity<InputState>,
    query: String,
    focus: FocusHandle,
    scroll: ScrollHandle,
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
        let max_concurrent = input(s.max_concurrent.to_string(), "1", window, cx, &mut subs);
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
        // Settings can change elsewhere (sidebar speed menu, theme toggle):
        // pick those up so a later commit doesn't revert them.
        subs.push(cx.observe_in(&AppStore::global(cx), window, |this, store, window, cx| {
            let new = store.read(cx).settings().clone();
            if new == this.draft {
                return;
            }
            let old = std::mem::replace(&mut this.draft, new.clone());
            if old.max_download_limit != new.max_download_limit {
                this.max_down.update(cx, |s, cx| s.set_value(rate_text(new.max_download_limit), window, cx));
            }
            if old.max_upload_limit != new.max_upload_limit {
                this.max_up.update(cx, |s, cx| s.set_value(rate_text(new.max_upload_limit), window, cx));
            }
            cx.notify();
        }));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search settings…"));
        subs.push(cx.subscribe_in(&search, window, |this, state, ev, _, cx| {
            if matches!(ev, InputEvent::Change) {
                this.query = state.read(cx).value().trim().to_lowercase();
                this.scroll.set_offset(point(dp(0.), dp(0.)));
                cx.notify();
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
            section: Section::General,
            search,
            query: String::new(),
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            _subs: subs,
        };
        this.sync_rules(window, cx);
        this
    }

    pub fn select(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        self.section = section;
        if !self.query.is_empty() {
            self.search.update(cx, |s, cx| s.set_value("", window, cx));
            self.query.clear();
        }
        self.scroll.set_offset(point(dp(0.), dp(0.)));
        cx.notify();
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
        crate::ui::kit::set_ui_scale(s.ui_scale);
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

    fn card(rows: impl IntoIterator<Item = AnyElement>, cx: &App) -> Div {
        let theme = cx.theme();
        v_flex()
            .rounded(dp(8.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .children(rows.into_iter().enumerate().map(|(i, r)| {
                div().when(i > 0, |this| this.border_t_1().border_color(theme.border)).child(r)
            }))
    }

    fn row(title: &str, description: Option<&str>, keywords: &str, control: impl IntoElement, cx: &App) -> Entry {
        let theme = cx.theme();
        let haystack = format!("{title} {} {keywords}", description.unwrap_or_default()).to_lowercase();
        let el = h_flex()
            .min_h(dp(56.))
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
            .into_any_element();
        Entry { haystack, el }
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
                            }).pressable(ElementId::Name(format!("rule-kind-{id}").into())),
                    )
                    .child(Button::new(ElementId::Name(format!("rule-del-{id}").into())).ghost().small().icon(Icon::new(Lucide::Trash)).on_click(
                        cx.listener({
                            let id = id.clone();
                            move |this, _, window, cx| {
                                this.edit(window, cx, |s| s.schedule.retain(|r| r.id != id));
                                this.sync_rules(window, cx);
                            }
                        }),
                    ).pressable(ElementId::Name(format!("rule-del-{id}").into()))),
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
                    .child(div().w(dp(72.)).child(Input::new(&row.start).small()))
                    .child(div().text_color(theme.muted_foreground).child("to"))
                    .child(div().w(dp(72.)).child(Input::new(&row.end).small()))
                    .when(matches!(action, ScheduleAction::Limit { .. }), |this| {
                        this.child(div().text_color(theme.muted_foreground).child("max"))
                            .child(div().w(dp(100.)).child(Input::new(&row.limit).small()))
                    }),
            )
            .into_any_element()
    }
}

impl SettingsView {
    /// The rows of one section.
    fn entries(&self, section: Section, cx: &mut Context<Self>) -> Vec<Entry> {
        let theme = cx.theme().clone();
        let s = &self.draft;
        let narrow = |e: &Entity<InputState>| div().w(dp(120.)).child(Input::new(e).small());
        let wide = |e: &Entity<InputState>| div().w(dp(260.)).child(Input::new(e).small());
        match section {
            Section::General => {
                let mut dir = s.download_dir.display().to_string();
                // Shorten the home folder to `~` (USERPROFILE on Windows). An unset
                // variable must not be replaced: "".replace would hit every char.
                if let Ok(home) = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                    && !home.is_empty()
                    && dir.starts_with(&home)
                {
                    dir = format!("~{}", &dir[home.len()..]);
                }
                let mut rows = vec![
                    Self::row(
                        "Download folder",
                        Some(&dir),
                        "location path directory save",
                        Button::new("pick-download-dir").small().label("Change…").on_click(cx.listener(|this, _, window, cx| this.pick_dir(window, cx))).pressable("pick-download-dir"),
                        cx,
                    ),
                    Self::row("Save in category folders", Some("Video, Music, Documents… inside the download folder"), "organize sort store", self.switch("category-folders", s.category_folders, cx, |s, v| s.category_folders = v), cx),
                    Self::row("Notify when downloads finish", None, "notification alert complete", self.switch("notify", s.notify_on_complete, cx, |s, v| s.notify_on_complete = v), cx),
                ];
                // Without a tray there's no way back to a closed window, so closing quits.
                if crate::platform::HAS_TRAY {
                    rows.push(Self::row("Keep running when the window closes", None, "background quit close tray", self.switch("close-to-tray", s.close_to_tray, cx, |s, v| s.close_to_tray = v), cx));
                }
                rows
            }
            Section::Appearance => {
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
                const SIZES: [(Option<f32>, &str); 6] = [(None, "Auto"), (Some(0.9), "90%"), (Some(1.0), "100%"), (Some(1.1), "110%"), (Some(1.25), "125%"), (Some(1.5), "150%")];
                let size_picker = SIZES
                    .iter()
                    .enumerate()
                    .fold(ButtonGroup::new("ui-scale").small().outline(), |group, (i, (v, label))| {
                        group.child(Button::new(("ui-scale", i)).label(*label).selected(s.ui_scale == *v))
                    })
                    .on_click(cx.listener(|this, ix: &Vec<usize>, window, cx| {
                        let v = ix.first().and_then(|i| SIZES.get(*i)).and_then(|(v, _)| *v);
                        this.edit(window, cx, |s| s.ui_scale = v);
                    }));
                let auto = format!("Auto follows the system ({:.0}%)", crate::platform::system_ui_scale() * 100.);
                let mut rows = vec![
                    Self::row("Theme", Some("Follow the system, or always light or dark"), "appearance dark mode light mode color", theme_picker, cx),
                    Self::row("Interface size", Some(&auto), "scale zoom font text size bigger smaller dpi", size_picker, cx),
                ];
                if crate::platform::HAS_TRAY {
                    rows.push(Self::row(if crate::platform::IS_MAC { "Show in menu bar" } else { "Show in system tray" }, Some("Live speed and quick controls"), "tray status icon", self.switch("show-tray", s.show_tray, cx, |s, v| s.show_tray = v), cx));
                }
                rows
            }
            Section::Connections => vec![
                Self::row("Concurrent downloads", Some("How many downloads run at once; the rest wait in the queue"), "parallel simultaneous max", narrow(&self.max_concurrent), cx),
                Self::row("Connections per download", Some("Splits a file into this many segments"), "split segments threads", narrow(&self.split), cx),
                Self::row("Connections per server", None, "max host", narrow(&self.per_server), cx),
                Self::row("User agent", None, "browser header ua", wide(&self.user_agent), cx),
                Self::row("Proxy", Some("Used for every protocol"), "http socks network", wide(&self.proxy), cx),
            ],
            Section::Bandwidth => vec![
                Self::row("Download limit", Some("Empty for unlimited · 500K, 2M"), "speed rate throttle cap", narrow(&self.max_down), cx),
                Self::row("Upload limit", Some("Applies to torrents"), "speed rate throttle cap", narrow(&self.max_up), cx),
                Self::row("Seed ratio", Some("Stop seeding at this upload/download ratio"), "torrent bittorrent", narrow(&self.seed_ratio), cx),
            ],
            Section::Schedule => {
                let mut rows: Vec<Entry> = s
                    .schedule
                    .iter()
                    .map(|r| Entry {
                        haystack: format!("{} schedule rule time {}", r.name, r.action.label()).to_lowercase(),
                        el: self.render_rule(r, cx),
                    })
                    .collect();
                rows.push(Entry {
                    haystack: "add rule schedule time limit pause window".into(),
                    el: h_flex()
                        .px_4()
                        .py_2()
                        .child(Button::new("add-rule").ghost().small().icon(Icon::new(Lucide::Plus)).label("Add rule").on_click(cx.listener(|this, _, window, cx| {
                            this.edit(window, cx, |s| s.schedule.push(ScheduleRule::new("Work hours", ScheduleAction::Limit { down: 512 * 1024, up: 0 })));
                            this.sync_rules(window, cx);
                        })).pressable("add-rule"))
                        .into_any_element(),
                });
                rows
            }
            Section::Browser => {
                let token = s.bridge.token.clone();
                let ext_dir = crate::platform::extension_dir();
                vec![
                    Self::row("Catch browser downloads", Some("Requires the YADM browser extension"), "bridge chrome firefox intercept", self.switch("bridge", s.bridge.enabled, cx, |s, v| s.bridge.enabled = v), cx),
                    Self::row("Start without asking", Some("Otherwise YADM shows the New download dialog"), "auto start confirm", self.switch("bridge-auto", s.bridge.auto_start, cx, |s, v| s.bridge.auto_start = v), cx),
                    Self::row("Port", None, "bridge extension network", narrow(&self.port), cx),
                    Self::row(
                        "Pairing token",
                        Some("Paste this into the extension's options"),
                        "extension secret key",
                        h_flex()
                            .gap_1()
                            .child(div().px_2().py_1().rounded(dp(4.)).bg(theme.muted).font_family(theme.mono_font_family.clone()).text_xs().child(token.clone()))
                            .child(Button::new("copy-token").ghost().small().icon(Icon::new(Lucide::Copy)).tooltip("Copy").on_click(move |_, window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(token.clone()));
                                window.push_notification(Notification::info("Token copied"), cx);
                            }).pressable("copy-token"))
                            .child(Button::new("regen-token").ghost().small().icon(Icon::new(Lucide::RefreshCw)).tooltip("Generate a new token").on_click(
                                cx.listener(|this, _, window, cx| this.edit(window, cx, |s| s.bridge.token = new_token())),
                            ).pressable("regen-token")),
                        cx,
                    ),
                    Self::row(
                        "Install the extension",
                        Some("Chrome: chrome://extensions → Developer mode → Load unpacked"),
                        "chrome setup",
                        Button::new("reveal-ext").small().label("Show folder").on_click(move |_, _, cx| cx.reveal_path(&ext_dir)).pressable("reveal-ext"),
                        cx,
                    ),
                ]
            }
            Section::Clipboard => vec![Self::row(
                "Offer to download copied links",
                Some("Watches the clipboard for links to files and magnets"),
                "monitor paste url magnet",
                self.switch("clipboard", s.clipboard_monitor, cx, |s, v| s.clipboard_monitor = v),
                cx,
            )],
            Section::Advanced => {
                let data_dir = yadm_core::Paths::system().data_dir;
                let log = yadm_core::Paths::system().engine_dir().join("aria2.log");
                vec![
                    Entry {
                        haystack: "aria2 options engine global min-split-size advanced".into(),
                        el: v_flex()
                            .px_4()
                            .py_3()
                            .gap_2()
                            .child(div().text_sm().child("aria2 options"))
                            .child(div().text_xs().text_color(theme.muted_foreground).child("Applied to the engine globally, e.g. min-split-size=4M. RPC options are ignored."))
                            .child(Textarea::new(&self.aria2_options).h(dp(90.)))
                            .into_any_element(),
                    },
                    Self::row(
                        "Engine files",
                        None,
                        "log data folder debug",
                        h_flex()
                            .gap_1()
                            .child(Button::new("open-log").small().label("Engine log").on_click(move |_, _, cx| cx.open_with_system(&log)).pressable("open-log"))
                            .child(Button::new("open-data").small().label("Data folder").on_click(move |_, _, cx| cx.reveal_path(&data_dir)).pressable("open-data")),
                        cx,
                    ),
                ]
            }
        }
    }

    /// Entries of `section` matching the current query (all when it's empty).
    fn matching(&self, section: Section, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let q = self.query.clone();
        let section_hit = section.label().to_lowercase().contains(&q);
        self.entries(section, cx)
            .into_iter()
            .filter(|e| q.is_empty() || section_hit || e.haystack.contains(&q))
            .map(|e| e.el)
            .collect()
    }

    fn match_count(&self, section: Section, cx: &mut Context<Self>) -> usize {
        let q = &self.query;
        if section.label().to_lowercase().contains(q.as_str()) {
            return self.entries(section, cx).len();
        }
        self.entries(section, cx).iter().filter(|e| e.haystack.contains(q.as_str())).count()
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let searching = !self.query.is_empty();

        let back = h_flex()
            .id("settings-back")
            .mx_2()
            .h(dp(30.))
            .px_2()
            .gap_2()
            .rounded(dp(6.))
            .cursor_pointer()
            .text_color(theme.muted_foreground)
            .hover(|this| this.bg(theme.sidebar_accent).text_color(theme.foreground))
            .child(Icon::new(Lucide::ChevronLeft).small())
            .child("Back to app")
            .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Close)))
            .pressable("settings-back");

        let search = div().px_2().pt_2().child(
            Input::new(&self.search)
                .small()
                .prefix(Icon::new(Lucide::Search).xsmall().text_color(theme.muted_foreground))
                .cleanable(true),
        );

        let mut groups = v_flex().px_2().pb_2();
        for (label, sections) in Section::GROUPS {
            groups = groups.child(
                div()
                    .px_2()
                    .pt_4()
                    .pb_1()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.muted_foreground)
                    .child(label),
            );
            for &section in sections {
                let hits = if searching { self.match_count(section, cx) } else { 0 };
                let active = !searching && self.section == section;
                let dim = searching && hits == 0;
                groups = groups.child(
                    h_flex()
                        .id(ElementId::Name(format!("settings-nav-{}", section.label()).into()))
                        .h(dp(30.))
                        .px_2()
                        .gap_2()
                        .rounded(dp(6.))
                        .cursor_pointer()
                        .text_color(if active { theme.sidebar_accent_foreground } else { theme.sidebar_foreground })
                        .when(active, |this| this.bg(theme.sidebar_accent))
                        .when(dim, |this| this.opacity(0.45))
                        .hover(|this| this.bg(theme.sidebar_accent))
                        .child(Icon::new(section.icon()).small().text_color(theme.muted_foreground))
                        .child(div().flex_1().truncate().child(section.label()))
                        .when(searching && hits > 0, |this| this.child(div().text_xs().text_color(theme.muted_foreground).child(hits.to_string())))
                        .on_click(cx.listener(move |this, _, window, cx| this.select(section, window, cx)))
                        .pressable(ElementId::Name(format!("settings-nav-{}", section.label()).into())),
                );
            }
        }

        v_flex()
            .w(dp(248.))
            .h_full()
            .flex_shrink_0()
            .bg(theme.sidebar)
            .child(window_drag_area("settings-titlebar"))
            .child(back)
            .child(search)
            .child(div().id("settings-nav-scroll").flex_1().overflow_y_scroll().child(groups))
            .into_any_element()
    }

    fn render_page(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let column = v_flex().w_full().max_w(dp(720.)).mx_auto().px_6().pt_10().pb_8().when_some(self.error.clone(), |this, e| {
            this.child(div().mb_6().p_2().rounded(dp(6.)).bg(theme.danger.opacity(0.1)).text_sm().text_color(theme.danger).child(e))
        });

        if self.query.is_empty() {
            let section = self.section;
            let rows = self.matching(section, cx);
            return column
                .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child(section.label()))
                .child(div().mt_1().mb_8().text_sm().text_color(theme.muted_foreground).child(section.blurb()))
                .child(Self::card(rows, cx));
        }

        let results: Vec<(Section, Vec<AnyElement>)> =
            Section::all().map(|s| (s, self.matching(s, cx))).filter(|(_, rows)| !rows.is_empty()).collect();
        let total: usize = results.iter().map(|(_, r)| r.len()).sum();
        let raw_query = self.search.read(cx).value().trim().to_string();
        let column = column.child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child("Search")).child(
            div().mt_1().mb_8().text_sm().text_color(theme.muted_foreground).child(match total {
                0 => format!("No settings match “{raw_query}”"),
                1 => format!("1 result for “{raw_query}”"),
                n => format!("{n} results for “{raw_query}”"),
            }),
        );
        column.child(v_flex().gap_8().children(results.into_iter().map(|(section, rows)| {
            v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .id(ElementId::Name(format!("settings-result-{}", section.label()).into()))
                        .gap_1p5()
                        .cursor_pointer()
                        .child(Icon::new(section.icon()).xsmall().text_color(theme.muted_foreground))
                        .child(section_label(section.label(), cx))
                        .on_click(cx.listener(move |this, _, window, cx| this.select(section, window, cx))),
                )
                .child(Self::card(rows, cx))
        })))
    }
}

impl EventEmitter<SettingsEvent> for SettingsView {}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        h_flex()
            .key_context("Settings")
            .track_focus(&self.focus)
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
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| this.search.update(cx, |s, cx| s.focus(window, cx))))
            .on_action(cx.listener(|this, _: &CloseOverlay, window, cx| {
                if this.query.is_empty() {
                    cx.emit(SettingsEvent::Close);
                } else {
                    this.search.update(cx, |s, cx| s.set_value("", window, cx));
                    this.query.clear();
                    this.focus.focus(window, cx);
                    cx.notify();
                }
            }))
            .child(self.render_sidebar(cx))
            .child(
                div().flex_1().min_w_0().h_full().py_2().pr_2().child(
                    div()
                        .id("settings-scroll")
                        .size_full()
                        .rounded(dp(10.))
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .child(self.render_page(cx)),
                ),
            )
    }
}
