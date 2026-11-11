//! Left navigation, Linear style: workspace header, primary views, categories.

use super::kit::{category_icon, state_color, state_icon, window_drag_area};
use super::root::{AppView, Nav};
use crate::actions::{CycleTheme, HideWindow, NewDownload, OpenCommandPalette, OpenDownloadsFolder, OpenSettings, Quit};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::DropdownMenu as _;
use crate::theme::palette;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use super::kit::PressableExt as _;
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
        let id = id.into();
        h_flex()
            .id(id.clone())
            .h(px(30.))
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
            .pressable(id)
            .into_any_element()
    }

    pub(super) fn render_sidebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let snap = self.store.read(cx).snapshot().clone();
        let p = palette(cx);
        let count_state = |s: State| snap.downloads.iter().filter(|d| d.state == s).count();
        let count_cat = |c: Category| snap.downloads.iter().filter(|d| d.category == c).count();
        let theme = cx.theme().clone();

        let titlebar = window_drag_area("sidebar-titlebar");

        // Linear's top row: workspace menu on the left, search and compose on the right.
        let app_menu = Button::new("app-menu")
            .ghost()
            .small()
            .child(
                h_flex()
                    .gap_1()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("YADM"))
                    .child(Icon::new(Lucide::ChevronDown).xsmall().text_color(theme.muted_foreground)),
            )
            .dropdown_menu(|menu, _, _| {
                menu.menu("Settings…", Box::new(OpenSettings))
                    .menu("Switch theme", Box::new(CycleTheme))
                    .menu("Open downloads folder", Box::new(OpenDownloadsFolder))
                    .separator()
                    .menu("Hide YADM", Box::new(HideWindow))
                    .menu("Quit YADM", Box::new(Quit))
            }).pressable("app-menu");
        let top_row = h_flex()
            .px_2()
            .h(px(34.))
            .gap_0p5()
            .child(app_menu)
            .child(div().flex_1())
            .child(
                Button::new("sidebar-search")
                    .ghost()
                    .small()
                    .icon(Icon::new(Lucide::Search))
                    .tooltip("Search  ⌘K")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenCommandPalette), cx)).pressable("sidebar-search"),
            )
            .child(
                Button::new("sidebar-new")
                    .outline()
                    .small()
                    .icon(Icon::new(Lucide::Plus))
                    .tooltip("New download  ⌘N")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(NewDownload), cx)).pressable("sidebar-new"),
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
        // With nothing downloaded there's nothing to browse by category.
        let any_visible = Category::ALL.len() > hidden;
        let expanded = self.show_empty_categories;
        let toggle = (hidden > 0 && any_visible).then(|| {
            h_flex()
                .id("nav-cat-toggle")
                .h(px(30.))
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
                .pressable("nav-cat-toggle")
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
            .child(
                h_flex()
                    .id("engine-status")
                    .px_2()
                    .h(px(26.))
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().size(px(6.)).rounded_full().bg(dot))
                    .child(div().flex_1().truncate().child(engine_label.clone()))
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(engine_label.clone()).build(window, cx)),
            );

        v_flex()
            .w(px(248.))
            .h_full()
            .flex_shrink_0()
            .bg(theme.sidebar)
            .child(titlebar)
            .child(top_row)
            .child(
                v_flex()
                    .id("sidebar-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(group_label("Views"))
                    .child(views)
                    .when(any_visible, |this| this.child(group_label("Categories")).child(categories))
                    .children(schedule),
            )
            .child(footer)
            .into_any_element()
    }
}
