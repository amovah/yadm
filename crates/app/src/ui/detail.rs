//! Right-hand detail panel, like a Linear issue sidebar.

use super::kit::{category_icon, progress_bar, property, section_label, state_color, status_glyph};
use super::root::AppView;
use crate::actions::*;
use crate::theme::palette;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use crate::ui::kit::dp;
use super::kit::PressableExt as _;
use yadm_core::{Category, Command, Download, DownloadKind, State, format};

gpui_kit::actions!(yadm_detail, [LimitNone, Limit256K, Limit1M, Limit5M]);

fn ts(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.with_timezone(&chrono::Local).format("%b %-d, %Y %H:%M").to_string())
        .unwrap_or_default()
}

impl AppView {
    fn set_limit(&mut self, limit: Option<u64>, cx: &mut Context<Self>) {
        if let Some(id) = self.cursor.clone() {
            self.store.read(cx).send(Command::SetSpeedLimit { id, limit });
        }
    }

    fn render_files(&self, d: &Download, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let multi = d.files.len() > 1 && d.state.is_live();
        v_flex()
            .gap_1()
            .children(d.files.iter().map(|f| {
                let name = std::path::Path::new(&f.path)
                    .strip_prefix(&d.dir)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| f.path.clone());
                let pct = if f.length > 0 { f.completed as f32 / f.length as f32 } else { 0. };
                let (id, index, selected) = (d.id.clone(), f.index, f.selected);
                let all: Vec<(u64, bool)> = d.files.iter().map(|f| (f.index, f.selected)).collect();
                v_flex()
                    .id(ElementId::Name(format!("file-{}-{}", d.id, f.index).into()))
                    .gap_1()
                    .py_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .when(multi, |this| {
                                this.child(
                                    gpui_kit::component::checkbox::Checkbox::new(ElementId::Name(format!("sel-{}-{}", id, index).into()))
                                        .checked(selected)
                                        .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                                            let indexes: Vec<u64> = all
                                                .iter()
                                                .filter(|(i, s)| if *i == index { *checked } else { *s })
                                                .map(|(i, _)| *i)
                                                .collect();
                                            if !indexes.is_empty() {
                                                this.store.read(cx).send(Command::SelectFiles { id: id.clone(), indexes });
                                            }
                                        })),
                                )
                            })
                            .child(div().flex_1().truncate().text_xs().child(if name.is_empty() { "(pending)".into() } else { name }))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(format::bytes(f.length))),
                    )
                    .child(progress_bar(pct, theme.progress_bar, cx))
            }))
            .into_any_element()
    }

    pub(super) fn render_detail(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let p = palette(cx);
        let store = self.store.read(cx);
        let Some(d) = self.cursor.as_ref().and_then(|id| store.download(id)).cloned() else {
            return div().into_any_element();
        };

        let primary_action = match d.state {
            State::Active | State::Queued => Some(("Pause", Lucide::Pause)),
            State::Paused => Some(("Resume", Lucide::Play)),
            State::Failed => Some(("Retry", Lucide::RotateCw)),
            _ => None,
        };

        let header = v_flex()
            .gap_3()
            .p_4()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_2()
                    .child(status_glyph(&d, cx))
                    .child(div().text_xs().text_color(state_color(d.state, &p, cx)).child(d.state.label()))
                    .child(div().flex_1())
                    .child(
                        Button::new("detail-close")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(Lucide::X))
                            .tooltip("Close  Esc")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.detail_open = false;
                                cx.notify();
                            })).pressable("detail-close"),
                    ),
            )
            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).line_height(rems(1.4)).child(d.name.clone()))
            .when(d.total > 0 && d.state != State::Completed, |this| {
                this.child(
                    v_flex()
                        .gap_1()
                        .child(progress_bar(d.progress(), state_color(d.state, &p, cx), cx))
                        .child(
                            h_flex()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!("{} of {}", format::bytes(d.completed), format::bytes(d.total)))
                                .child(div().flex_1())
                                .child(format!("{:.1}%", d.progress() * 100.)),
                        ),
                )
            })
            .child(
                h_flex()
                    .gap_1()
                    .when_some(primary_action, |this, (label, icon)| {
                        let action: Box<dyn Action> = if d.state == State::Failed { Box::new(RetrySelected) } else { Box::new(PauseResume) };
                        this.child(
                            Button::new("detail-primary")
                                .small()
                                .icon(Icon::new(icon))
                                .label(label)
                                .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx)).pressable("detail-primary"),
                        )
                    })
                    .when(d.state == State::Completed, |this| {
                        this.child(
                            Button::new("detail-open")
                                .small()
                                .icon(Icon::new(Lucide::ExternalLink))
                                .label("Open")
                                .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSelected), cx)).pressable("detail-open"),
                        )
                    })
                    .child(
                        Button::new("detail-reveal")
                            .small()
                            .ghost()
                            .icon(Icon::new(Lucide::FolderOpen))
                            .tooltip(format!("{}  {}", crate::platform::REVEAL, crate::platform::shortcut("⌘O")))
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(RevealSelected), cx)).pressable("detail-reveal"),
                    )
                    .child(
                        Button::new("detail-copy")
                            .small()
                            .ghost()
                            .icon(Icon::new(Lucide::Link))
                            .tooltip("Copy source URL")
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(CopySourceUrl), cx)).pressable("detail-copy"),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("detail-remove")
                            .small()
                            .ghost()
                            .icon(Icon::new(if d.state == State::Completed { Lucide::ListX } else { Lucide::CircleX }))
                            .tooltip(if d.state == State::Completed { "Remove from list" } else { "Cancel download" })
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(RemoveSelected), cx)).pressable("detail-remove"),
                    ),
            );

        let limit_label = d.speed_limit.map(format::speed).unwrap_or_else(|| "Unlimited".into());
        let category = d.category;
        let cat_id = d.id.clone();
        let props = v_flex()
            .gap_0p5()
            .p_4()
            .child(section_label("Properties", cx))
            .when(d.state == State::Active, |this| {
                this.child(property(
                    "Speed",
                    // Only torrents upload; for HTTP/FTP the upload side is always zero.
                    if matches!(d.kind, DownloadKind::Torrent | DownloadKind::Magnet) {
                        format!("↓ {}   ↑ {}", format::speed(d.dl_speed), format::speed(d.ul_speed))
                    } else {
                        format::speed(d.dl_speed)
                    },
                    cx,
                ))
                    .child(property("Time left", d.eta_secs().map(format::duration).unwrap_or_else(|| "—".into()), cx))
                    .child(property(
                        if matches!(d.kind, DownloadKind::Torrent | DownloadKind::Magnet) { "Peers" } else { "Connections" },
                        match d.seeders {
                            Some(s) => format!("{} ({} seeders)", d.connections, s),
                            None => d.connections.to_string(),
                        },
                        cx,
                    ))
            })
            .child(property("Size", if d.total > 0 { format::bytes(d.total) } else { "Unknown".into() }, cx))
            .when(d.uploaded > 0, |this| this.child(property("Uploaded", format::bytes(d.uploaded), cx)))
            .child(property(
                "Category",
                Button::new("detail-category")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(category_icon(category)))
                    .label(category.label())
                    .dropdown_menu(move |menu, _, _| {
                        let mut menu = menu;
                        for c in Category::ALL {
                            menu = menu.menu_with_check(c.label(), c == category, Box::new(SetCategoryAction { id: cat_id.clone(), category: c.as_str().into() }));
                        }
                        menu
                    }).pressable("detail-category"),
                cx,
            ))
            .when(d.state.is_live(), |this| {
                this.child(property(
                    "Speed limit",
                    Button::new("detail-limit").ghost().xsmall().icon(Icon::new(Lucide::Gauge)).label(limit_label).dropdown_menu(|menu, _, _| {
                        menu.menu("Unlimited", Box::new(LimitNone))
                            .menu("256 KB/s", Box::new(Limit256K))
                            .menu("1 MB/s", Box::new(Limit1M))
                            .menu("5 MB/s", Box::new(Limit5M))
                    }).pressable("detail-limit"),
                    cx,
                ))
            })
            .child(property(
                "Location",
                div()
                    .id("detail-location")
                    .text_xs()
                    .cursor_pointer()
                    .text_color(theme.link)
                    .child(d.dir.clone())
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(RevealSelected), cx)),
                cx,
            ))
            .when_some(d.uris.first().cloned(), |this, u| {
                this.child(property("Source", div().text_xs().text_color(theme.muted_foreground).child(u), cx))
            })
            .child(property("Added", ts(d.added_at), cx))
            .when_some(d.finished_at, |this, f| this.child(property("Finished", ts(f), cx)))
            .when_some(d.error.clone(), |this, e| {
                this.child(
                    div()
                        .mt_2()
                        .p_2()
                        .rounded(dp(6.))
                        .border_1()
                        .border_color(p.red.opacity(0.3))
                        .bg(p.red.opacity(0.08))
                        .text_xs()
                        .text_color(p.red)
                        .child(e),
                )
            });

        // Only multi-file torrents need a file list (to pick what to download).
        let files = (matches!(d.kind, DownloadKind::Torrent | DownloadKind::Magnet) && d.files.len() > 1)
            .then(|| v_flex().px_4().pb_4().gap_2().child(section_label("Files", cx)).child(self.render_files(&d, cx)));

        v_flex()
            .w(dp(400.))
            .h_full()
            .flex_shrink_0()
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.background)
            .on_action(cx.listener(|this, _: &LimitNone, _, cx| this.set_limit(None, cx)))
            .on_action(cx.listener(|this, _: &Limit256K, _, cx| this.set_limit(Some(256 * 1024), cx)))
            .on_action(cx.listener(|this, _: &Limit1M, _, cx| this.set_limit(Some(1024 * 1024), cx)))
            .on_action(cx.listener(|this, _: &Limit5M, _, cx| this.set_limit(Some(5 * 1024 * 1024), cx)))
            .on_action(cx.listener(|this, a: &SetCategoryAction, _, cx| {
                this.store.read(cx).send(Command::SetCategory { id: a.id.clone(), category: Category::parse(&a.category) });
            }))
            .child(
                v_flex()
                    .id("detail-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(header)
                    .child(props)
                    .children(files),
            )
            .into_any_element()
    }
}

/// Sets a download's category from the detail panel menu.
#[derive(Clone, PartialEq, Debug, serde::Deserialize, schemars::JsonSchema, Action)]
#[action(namespace = yadm_detail)]
pub struct SetCategoryAction {
    pub id: String,
    pub category: String,
}
