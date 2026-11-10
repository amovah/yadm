//! Left navigation, Linear style: workspace header, primary views, categories.

use super::kit::{category_icon, kbd, state_color, state_icon};
use super::root::{AppView, Nav};
use crate::actions::NewDownload;
use crate::theme::palette;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use yadm_core::format;
use yadm_core::{Category, EngineStatus, State};

impl AppView {
    fn nav_item(
        &self,
        id: impl Into<ElementId>,
        icon: Lucide,
        icon_color: Option<Hsla>,
        label: SharedString,
        count: Option<usize>,
        nav: Nav,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.nav == nav;
        let theme = cx.theme();
        h_flex()
            .id(id)
            .h(px(28.))
            .px_2()
            .gap_2()
            .rounded(px(6.))
            .cursor_pointer()
            .text_color(if active { theme.sidebar_accent_foreground } else { theme.sidebar_foreground })
            .when(active, |this| this.bg(theme.sidebar_accent))
            .hover(|this| this.bg(theme.sidebar_accent))
            .child(
                Icon::new(icon)
                    .small()
                    .text_color(icon_color.unwrap_or(theme.muted_foreground)),
            )
            .child(div().flex_1().truncate().child(label))
            .when_some(count, |this, c| {
                this.child(div().text_xs().text_color(theme.muted_foreground).child(c.to_string()))
            })
            .on_click(cx.listener(move |this, _, window, cx| this.set_nav(nav, window, cx)))
            .into_any_element()
    }

    pub(super) fn render_sidebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let snap = self.store.read(cx).snapshot().clone();
        let p = palette(cx);
        let count_state = |s: State| snap.downloads.iter().filter(|d| d.state == s).count();
        let count_cat = |c: Category| snap.downloads.iter().filter(|d| d.category == c).count();
        let theme = cx.theme().clone();

        // Own row for the macOS traffic lights; doubles as the window drag area.
        let titlebar = div()
            .id("sidebar-titlebar")
            .h(px(40.))
            .flex_shrink_0()
            .on_mouse_down(MouseButton::Left, |ev: &MouseDownEvent, window, _| {
                if ev.click_count >= 2 {
                    window.titlebar_double_click();
                } else {
                    window.start_window_move();
                }
            });

        let header = h_flex()
            .h(px(36.))
            .px_4()
            .gap_2()
            .child(
                div()
                    .size(px(20.))
                    .rounded(px(5.))
                    .bg(p.accent)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(Lucide::CircleArrowDown).xsmall().text_color(white())),
            )
            .child(div().font_weight(FontWeight::SEMIBOLD).child("YADM"));

        let new_button = h_flex()
            .id("sidebar-new")
            .mx_2()
            .h(px(30.))
            .px_2()
            .gap_2()
            .rounded(px(6.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .cursor_pointer()
            .hover(|this| this.bg(theme.accent))
            .child(Icon::new(Lucide::SquarePen).small().text_color(theme.muted_foreground))
            .child(div().flex_1().child("New download"))
            .child(kbd("⌘ N", cx))
            .on_click(|_, window, cx| window.dispatch_action(Box::new(NewDownload), cx));

        let speed = h_flex()
            .mx_2()
            .px_2()
            .gap_3()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(
                h_flex()
                    .gap_1()
                    .child(Icon::new(Lucide::ArrowDown).xsmall().text_color(p.accent))
                    .child(format::speed(snap.global.dl_speed)),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(Icon::new(Lucide::ArrowUp).xsmall().text_color(p.green))
                    .child(format::speed(snap.global.ul_speed)),
            );

        let group_label = |label: &'static str| {
            div()
                .px_2()
                .pt_4()
                .pb_1()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child(label)
        };

        let mut view_items = vec![self.nav_item("nav-all", Lucide::Inbox, None, "All downloads".into(), Some(snap.downloads.len()).filter(|c| *c > 0), Nav::All, cx)];
        for s in State::GROUPS {
                let id: &'static str = match s {
                    State::Active => "nav-active",
                    State::Queued => "nav-queued",
                    State::Paused => "nav-paused",
                    State::Failed => "nav-failed",
                    _ => "nav-completed",
                };
            let color = state_color(s, &p, cx);
            view_items.push(self.nav_item(id, state_icon(s), Some(color), Nav::State(s).title().into(), Some(count_state(s)).filter(|c| *c > 0), Nav::State(s), cx));
        }
        let views = v_flex().px_2().gap_px().children(view_items);

        // Categories with downloads (or the one being viewed) are always listed;
        // empty ones hide behind a "Show N more" toggle.
        let mut cat_items = vec![];
        let mut hidden = 0;
        for c in Category::ALL {
            let n = count_cat(c);
            let visible = n > 0 || self.nav == Nav::Category(c);
            if !visible {
                hidden += 1;
                if !self.show_empty_categories {
                    continue;
                }
            }
            let id = ElementId::Name(format!("nav-cat-{}", c.as_str()).into());
            cat_items.push(self.nav_item(id, category_icon(c), None, c.label().into(), Some(n), Nav::Category(c), cx));
        }
        let expanded = self.show_empty_categories;
        let toggle = (hidden > 0).then(|| {
            h_flex()
                .id("nav-cat-toggle")
                .h(px(28.))
                .px_2()
                .gap_2()
                .rounded(px(6.))
                .cursor_pointer()
                .text_color(theme.muted_foreground)
                .hover(|this| this.bg(theme.sidebar_accent))
                .child(Icon::new(if expanded { Lucide::ChevronUp } else { Lucide::ChevronDown }).small())
                .child(if expanded { "Show less".to_string() } else { format!("Show {hidden} more") })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_empty_categories = !this.show_empty_categories;
                    cx.notify();
                }))
        });
        let categories = v_flex().px_2().gap_px().children(cat_items).children(toggle);

        let schedule = snap.schedule.clone().map(|label| {
            h_flex()
                .mx_2()
                .mt_3()
                .px_2()
                .py_1p5()
                .gap_2()
                .rounded(px(6.))
                .bg(theme.muted)
                .text_xs()
                .child(Icon::new(Lucide::CalendarClock).xsmall().text_color(p.yellow))
                .child(div().flex_1().truncate().child(label))
        });

        let (dot, engine_label) = match &snap.engine {
            EngineStatus::Running { version } => (p.green, format!("aria2 {version}")),
            EngineStatus::Starting => (p.yellow, "Starting engine…".to_string()),
            EngineStatus::Failed(e) => (p.red, e.clone()),
        };
        let footer = v_flex()
            .px_2()
            .pb_2()
            .gap_px()
            .child(self.nav_item("nav-settings", Lucide::Settings, None, "Settings".into(), None, Nav::Settings, cx))
            .child(
                h_flex()
                    .id("engine-status")
                    .px_2()
                    .h(px(24.))
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().size(px(6.)).rounded_full().bg(dot))
                    .child(div().flex_1().truncate().child(engine_label.clone()))
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(engine_label.clone()).build(window, cx)),
            );

        v_flex()
            .w(px(232.))
            .h_full()
            .flex_shrink_0()
            .bg(theme.sidebar)
            .child(titlebar)
            .child(header)
            .child(new_button)
            .child(div().h_2())
            .child(speed)
            .child(
                v_flex()
                    .id("sidebar-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(group_label("Views"))
                    .child(views)
                    .child(group_label("Categories"))
                    .child(categories)
                    .children(schedule),
            )
            .child(footer)
            .into_any_element()
    }
}
