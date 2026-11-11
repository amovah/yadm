//! Status bar along the bottom of the content panel: live speeds, queue
//! counts and the quick bandwidth limit menu.

use super::root::AppView;
use crate::actions::{OpenBandwidthSettings, SetDownloadLimit};
use crate::theme::palette;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use crate::ui::kit::dp;
use super::kit::PressableExt as _;
use yadm_core::format;

const LIMIT_PRESETS: [(&str, u64); 6] = [
    ("256 KB/s", 256),
    ("512 KB/s", 512),
    ("1 MB/s", 1024),
    ("2 MB/s", 2048),
    ("5 MB/s", 5120),
    ("10 MB/s", 10240),
];

impl AppView {
    pub(super) fn render_status_bar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let snap = self.store.read(cx).snapshot().clone();
        let limit = self.store.read(cx).settings().max_download_limit;
        let p = palette(cx);
        let theme = cx.theme().clone();

        let limit_button = Button::new("speed-limit")
            .ghost()
            .small()
            .icon(Icon::new(Lucide::Gauge).text_color(if limit > 0 { p.yellow } else { theme.muted_foreground }))
            .label(if limit > 0 { format!("Limit {}", format::speed(limit)) } else { "No limit".into() })
            .tooltip("Download speed limit")
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.menu_with_check("Unlimited", limit == 0, Box::new(SetDownloadLimit { bytes: 0 })).separator();
                for (label, kb) in LIMIT_PRESETS {
                    let bytes = kb * 1024;
                    menu = menu.menu_with_check(label, limit == bytes, Box::new(SetDownloadLimit { bytes }));
                }
                menu.separator().menu("Bandwidth settings…", Box::new(OpenBandwidthSettings))
            }).pressable("speed-limit");

        let stat = |icon: Lucide, color: Hsla, text: String| {
            h_flex().gap_1p5().child(Icon::new(icon).small().text_color(color)).child(text)
        };
        let counts = match (snap.global.active, snap.global.waiting) {
            (0, 0) => None,
            (a, 0) => Some(format!("{a} downloading")),
            (0, w) => Some(format!("{w} queued")),
            (a, w) => Some(format!("{a} downloading · {w} queued")),
        };

        h_flex()
            .h(dp(38.))
            .flex_shrink_0()
            .pl_4()
            .pr_2()
            .gap_5()
            .border_t_1()
            .border_color(theme.border)
            .text_sm()
            .child(stat(Lucide::ArrowDown, p.accent, format::speed(snap.global.dl_speed)))
            .child(stat(Lucide::ArrowUp, p.green, format::speed(snap.global.ul_speed)).text_color(theme.muted_foreground))
            .when_some(counts, |this, c| this.child(div().text_color(theme.muted_foreground).child(c)))
            .child(div().flex_1())
            .child(limit_button)
            .into_any_element()
    }
}
