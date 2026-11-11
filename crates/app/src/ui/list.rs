//! The grouped, virtualized download list with Linear-style rows.

use super::kit::{ROW_H, category_icon, chip, kbd, kbd_on, progress_bar, state_color, state_icon, status_glyph};
use super::root::{AppView, GroupBy, GroupKey, Nav, Row};
use crate::actions::{self, *};
use crate::theme::palette;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use crate::ui::kit::dp;
use super::kit::PressableExt as _;
use std::ops::Range;
use yadm_core::{Command, Download, State, format};

gpui_kit::actions!(yadm_list, [GroupByStatus, GroupByCategory, GroupByNone]);

impl AppView {
    fn render_header_row(&self, key: GroupKey, count: usize, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let theme = cx.theme();
        let (icon, color, label): (Lucide, Hsla, SharedString) = match key {
            GroupKey::State(s) => (state_icon(s), state_color(s, &p, cx), s.label().into()),
            GroupKey::Category(c) => (category_icon(c), theme.muted_foreground, c.label().into()),
        };
        let collapsed = self.collapsed.contains(&key);
        let id = ElementId::Name(format!("group-{key:?}").into());
        h_flex()
            .id(id)
            .w_full()
            .h(dp(ROW_H))
            .px_4()
            .gap_2()
            .bg(theme.list_head)
            .border_b_1()
            .border_color(theme.border)
            .cursor_pointer()
            .child(
                Icon::new(if collapsed { Lucide::ChevronRight } else { Lucide::ChevronDown })
                    .xsmall()
                    .text_color(theme.muted_foreground),
            )
            .child(Icon::new(icon).small().text_color(color))
            .child(div().font_weight(FontWeight::MEDIUM).child(label))
            .child(div().text_color(theme.muted_foreground).child(count.to_string()))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.collapsed.remove(&key) {
                    this.collapsed.insert(key);
                }
                this.rebuild_rows(cx);
                cx.notify();
            }))
            .into_any_element()
    }

    fn render_item_row(&self, d: &Download, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let theme = cx.theme().clone();
        let selected = self.selected.contains(&d.id);
        let is_cursor = self.cursor.as_deref() == Some(d.id.as_str());
        let id = d.id.clone();
        let now = chrono::Utc::now().timestamp_millis();

        let size = match (d.state, d.total) {
            (_, 0) => "—".to_string(),
            (State::Completed, t) => format::bytes(t),
            (_, t) => format!("{} / {}", format::bytes(d.completed), format::bytes(t)),
        };
        let speed_or_status: SharedString = match d.state {
            State::Active => d
                .eta_secs()
                .map(|e| format!("{} · {}", format::speed(d.dl_speed), format::duration(e)))
                .unwrap_or_else(|| format::speed(d.dl_speed))
                .into(),
            State::Failed => "Failed".into(),
            State::Paused => "Paused".into(),
            State::Queued => d.queue_pos.map(|q| format!("#{}", q + 1)).unwrap_or_else(|| "Queued".into()).into(),
            State::Completed => d.finished_at.map(|f| format::relative(f, now)).unwrap_or_default().into(),
            State::Removed => "".into(),
        };
        let subtitle = match (&d.error, d.host()) {
            (Some(e), _) if d.state == State::Failed => e.clone(),
            (_, Some(h)) => h,
            _ => String::new(),
        };
        let show_bar = matches!(d.state, State::Active | State::Paused | State::Queued) && d.total > 0;

        h_flex()
            .id(ElementId::Name(format!("row-{}", d.id).into()))
            .h(dp(ROW_H))
            .pl_4()
            .pr_4()
            .gap_3()
            .border_b_1()
            .border_color(theme.table_row_border)
            .cursor_default()
            .when(selected, |this| this.bg(theme.list_active))
            .when(is_cursor && !selected, |this| this.bg(theme.list_hover))
            .hover(|this| this.bg(theme.list_hover))
            .child(
                // Selection rail, like Linear's checkbox gutter.
                div()
                    .w(dp(2.))
                    .h(dp(20.))
                    .rounded_full()
                    .when(selected || is_cursor, |this| this.bg(p.accent)),
            )
            .child(div().w(dp(16.)).flex().justify_center().child(status_glyph(d, cx)))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .child(div().truncate().flex_shrink(1.).min_w(dp(40.)).child(d.name.clone()))
                    .child(
                        div()
                            .flex_shrink(1.)
                            .truncate()
                            .text_xs()
                            .text_color(if d.state == State::Failed { p.red } else { theme.muted_foreground })
                            .child(subtitle),
                    ),
            )
            .child(
                div()
                    .w(dp(96.))
                    .flex_shrink_0()
                    .when(show_bar, |this| this.child(progress_bar(d.progress(), state_color(d.state, &p, cx), cx))),
            )
            .child(
                div()
                    .w(dp(48.))
                    .flex_shrink_0()
                    .text_right()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .when(show_bar, |this| this.child(format!("{:.0}%", d.progress() * 100.))),
            )
            .child(div().w(dp(150.)).flex_shrink_0().text_right().text_xs().text_color(theme.muted_foreground).child(size))
            .child(div().w(dp(130.)).flex_shrink_0().text_right().text_xs().text_color(theme.muted_foreground).child(speed_or_status))
            .child(div().w(dp(96.)).flex_shrink_0().flex().justify_end().child(chip(d.category.label(), Some(category_icon(d.category)), cx)))
            .child(
                div()
                    .w(dp(36.))
                    .flex_shrink_0()
                    .text_right()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format::relative(d.added_at, now)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    if ev.modifiers.platform {
                        if !this.selected.remove(&id) {
                            this.selected.insert(id.clone());
                        }
                    } else if ev.modifiers.shift {
                        let anchor = this.cursor.clone().and_then(|c| this.order.iter().position(|o| *o == c));
                        let target = this.order.iter().position(|o| *o == id);
                        if let (Some(a), Some(b)) = (anchor, target) {
                            let (lo, hi) = (a.min(b), a.max(b));
                            this.selected.extend(this.order[lo..=hi].iter().cloned());
                        }
                    } else {
                        this.selected.clear();
                        this.detail_open = true;
                    }
                    this.cursor = Some(id.clone());
                    cx.notify();
                }),
            )
            .on_mouse_down(MouseButton::Right, {
                let id = d.id.clone();
                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                    // The context menu acts on the row under the pointer; keep a
                    // multi-selection only when the row is part of it.
                    this.focus.focus(window, cx);
                    if !this.selected.contains(&id) {
                        this.selected.clear();
                    }
                    this.cursor = Some(id.clone());
                    cx.notify();
                })
            })
            .context_menu({
                let d = d.clone();
                move |menu, _, _| {
                    let menu = match d.state {
                        State::Active | State::Queued => menu.menu_with_icon("Pause", Lucide::Pause, Box::new(PauseResume)),
                        State::Paused => menu.menu_with_icon("Resume", Lucide::Play, Box::new(PauseResume)),
                        State::Failed => menu.menu_with_icon("Retry", Lucide::RotateCw, Box::new(RetrySelected)),
                        _ => menu,
                    };
                    menu.when(d.state == State::Completed, |m| m.menu_with_icon("Open", Lucide::ExternalLink, Box::new(OpenSelected)))
                        .menu_with_icon("Open details", Lucide::PanelRight, Box::new(OpenDetails))
                        .menu_with_icon(crate::platform::REVEAL, Lucide::FolderOpen, Box::new(RevealSelected))
                        .menu_with_icon("Copy source URL", Lucide::Link, Box::new(CopySourceUrl))
                        .when(d.state == State::Queued, |m| {
                            m.separator()
                                .menu_with_icon("Move up", Lucide::ArrowUp, Box::new(MoveUp))
                                .menu_with_icon("Move down", Lucide::ArrowDown, Box::new(MoveDown))
                        })
                        .separator()
                        // Unfinished downloads have one way out: cancel, which also
                        // clears the partial files. Finished ones keep or drop the file.
                        .when(d.state != State::Completed, |m| {
                            m.menu_with_icon("Cancel download", Lucide::CircleX, Box::new(RemoveSelected))
                        })
                        .when(d.state == State::Completed, |m| {
                            m.menu_with_icon("Remove from list", Lucide::ListX, Box::new(RemoveSelected))
                                .menu_with_icon(
                                    if d.files.len() > 1 { "Delete files" } else { "Delete file" },
                                    Lucide::Trash,
                                    Box::new(RemoveSelectedWithFiles),
                                )
                        })
                }
            })
            .into_any_element()
    }

    fn render_rows(&mut self, range: Range<usize>, _window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let snap = self.store.read(cx).snapshot().clone();
        range
            .filter_map(|i| match self.rows.get(i)? {
                Row::Header { key, count } => Some(self.render_header_row(*key, *count, cx)),
                Row::Item(idx) => snap.downloads.get(*idx).map(|d| self.render_item_row(d, cx)),
            })
            .collect()
    }

    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let total = self.order.len();
        let group_label = match self.group_by {
            GroupBy::Status => "Status",
            GroupBy::Category => "Category",
            GroupBy::None => "No grouping",
        };
        let group_by = self.group_by;
        h_flex()
            .h(dp(48.))
            .flex_shrink_0()
            .px_4()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(div().font_weight(FontWeight::MEDIUM).child(self.nav.title()))
            .child(div().text_color(theme.muted_foreground).child(total.to_string()))
            .child(div().flex_1())
            .child(
                div().w(dp(220.)).child(
                    Input::new(&self.search)
                        .small()
                        .prefix(Icon::new(Lucide::Search).xsmall().text_color(theme.muted_foreground))
                        .cleanable(true),
                ),
            )
            .child(
                Button::new("group-by")
                    .ghost()
                    .small()
                    .icon(Icon::new(Lucide::Layers))
                    .label(group_label)
                    .dropdown_menu(move |menu, _, _| {
                        menu.menu_with_check("Status", group_by == GroupBy::Status, Box::new(GroupByStatus))
                            .menu_with_check("Category", group_by == GroupBy::Category, Box::new(GroupByCategory))
                            .menu_with_check("No grouping", group_by == GroupBy::None, Box::new(GroupByNone))
                    }).pressable("group-by"),
            )
            .child(
                Button::new("pause-all")
                    .ghost()
                    .small()
                    .icon(Icon::new(Lucide::Pause))
                    .tooltip(format!("Pause all  {}", crate::platform::shortcut("⌥⌘P")))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(PauseAll), cx)).pressable("pause-all"),
            )
            .child(
                Button::new("resume-all")
                    .ghost()
                    .small()
                    .icon(Icon::new(Lucide::Play))
                    .tooltip(format!("Resume all  {}", crate::platform::shortcut("⌥⌘R")))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ResumeAll), cx)).pressable("resume-all"),
            )
            .child(
                Button::new("clear-finished")
                    .ghost()
                    .small()
                    .icon(Icon::new(Lucide::ListX))
                    .tooltip("Clear completed")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ClearFinished), cx)).pressable("clear-finished"),
            )
            .child(
                Button::new("add")
                    .primary()
                    .small()
                    .icon(Icon::new(Lucide::Plus))
                    .label("Add")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(NewDownload), cx)).pressable("add"),
            )
            .into_any_element()
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let filtered = !self.query.is_empty() || self.nav != Nav::All;
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                div()
                    .size(dp(48.))
                    .rounded(dp(12.))
                    .border_1()
                    .border_color(theme.border)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(Lucide::ArrowDownToLine).large().text_color(theme.muted_foreground)),
            )
            .child(div().font_weight(FontWeight::MEDIUM).child(if filtered { "Nothing here" } else { "No downloads yet" }))
            .child(
                h_flex()
                    .gap_1()
                    .text_color(theme.muted_foreground)
                    .child("Paste a link with")
                    .child(kbd(&crate::platform::shortcut("⌘V"), cx))
                    .child("or create one with")
                    .child(kbd(&crate::platform::shortcut("⌘N"), cx)),
            )
            .when(!filtered, |this| {
                this.child(
                    Button::new("empty-add")
                        .primary()
                        .small()
                        .child(h_flex().gap_1p5().child("New download").child(kbd_on(&crate::platform::shortcut("⌘N"), theme.primary_foreground)))
                        .on_click(|_, window, cx| window.dispatch_action(Box::new(NewDownload), cx)).pressable("empty-add"),
                )
            })
            .into_any_element()
    }

    fn render_bulk_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.selected.len() < 2 {
            return None;
        }
        let theme = cx.theme();
        let ids: Vec<String> = self.selected.iter().cloned().collect();
        let (pause_ids, resume_ids) = (ids.clone(), ids.clone());
        Some(
            div().absolute().bottom_4().left_0().right_0().flex().justify_center().child(
                h_flex()
                    .gap_1()
                    .p_1()
                    .pl_3()
                    .rounded(dp(8.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.popover)
                    .shadow_lg()
                    .child(div().pr_2().text_sm().child(format!("{} selected", ids.len())))
                    .child(Button::new("bulk-pause").ghost().small().icon(Icon::new(Lucide::Pause)).label("Pause").on_click(cx.listener(
                        move |this, _, _, cx| this.store.read(cx).send(Command::Pause(pause_ids.clone())),
                    )).pressable("bulk-pause"))
                    .child(Button::new("bulk-resume").ghost().small().icon(Icon::new(Lucide::Play)).label("Resume").on_click(cx.listener(
                        move |this, _, _, cx| this.store.read(cx).send(Command::Resume(resume_ids.clone())),
                    )).pressable("bulk-resume"))
                    .child(Button::new("bulk-remove").ghost().small().icon(Icon::new(Lucide::Trash)).label("Remove").on_click(cx.listener(
                        move |this, _, window, cx| this.confirm_remove(ids.clone(), false, window, cx),
                    )).pressable("bulk-remove"))
                    .child(div().w_px().h(dp(16.)).bg(theme.border))
                    .child(Button::new("bulk-clear").ghost().small().icon(Icon::new(Lucide::X)).tooltip("Clear selection  Esc").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.selected.clear();
                            cx.notify();
                        },
                    )).pressable("bulk-clear")),
            )
            .into_any_element(),
        )
    }

    pub(super) fn render_list(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let toolbar = self.render_toolbar(cx);
        let body: AnyElement = if self.rows.is_empty() {
            self.render_empty(cx)
        } else {
            uniform_list("downloads", self.rows.len(), cx.processor(Self::render_rows))
                .track_scroll(&self.scroll)
                .flex_1()
                .into_any_element()
        };
        v_flex()
            .size_full()
            .relative()
            .key_context(actions::LIST_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &GroupByStatus, _, cx| this.set_group(GroupBy::Status, cx)))
            .on_action(cx.listener(|this, _: &GroupByCategory, _, cx| this.set_group(GroupBy::Category, cx)))
            .on_action(cx.listener(|this, _: &GroupByNone, _, cx| this.set_group(GroupBy::None, cx)))
            .child(toolbar)
            .child(body)
            .children(self.render_bulk_bar(cx))
    }

    fn set_group(&mut self, g: GroupBy, cx: &mut Context<Self>) {
        self.group_by = g;
        self.collapsed.clear();
        self.rebuild_rows(cx);
        cx.notify();
    }
}
