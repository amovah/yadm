//! Time-window rules that throttle or pause downloading.

use chrono::{Datelike, NaiveDateTime, NaiveTime, Timelike};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduleAction {
    /// Cap speeds (bytes/s, 0 = unlimited) while the window is active.
    Limit { down: u64, up: u64 },
    /// Pause everything while the window is active.
    Pause,
    /// Only download inside this window; pause everything outside it.
    DownloadOnly,
}

impl ScheduleAction {
    pub fn label(&self) -> &'static str {
        match self {
            ScheduleAction::Limit { .. } => "Limit speed",
            ScheduleAction::Pause => "Pause all",
            ScheduleAction::DownloadOnly => "Download only",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduleRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// Monday = index 0.
    pub days: [bool; 7],
    /// "HH:MM"
    pub start: String,
    /// "HH:MM"; a window ending before it starts crosses midnight.
    pub end: String,
    pub action: ScheduleAction,
}

impl ScheduleRule {
    pub fn new(name: impl Into<String>, action: ScheduleAction) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            enabled: true,
            days: [true; 7],
            start: "09:00".into(),
            end: "17:00".into(),
            action,
        }
    }

    fn parse(t: &str) -> Option<NaiveTime> {
        NaiveTime::parse_from_str(t.trim(), "%H:%M").ok()
    }

    /// Whether `now` falls inside this rule's window.
    pub fn is_active(&self, now: NaiveDateTime) -> bool {
        let (Some(start), Some(end)) = (Self::parse(&self.start), Self::parse(&self.end)) else {
            return false;
        };
        let t = now.time().with_second(0).unwrap_or(now.time());
        let wd = now.weekday().num_days_from_monday() as usize;
        if start <= end {
            self.days[wd] && t >= start && t < end
        } else {
            // Crosses midnight: the part after midnight belongs to the previous day's window.
            let prev = (wd + 6) % 7;
            (self.days[wd] && t >= start) || (self.days[prev] && t < end)
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Effective {
    pub paused: bool,
    /// Bytes/s; 0 = unlimited.
    pub down: u64,
    pub up: u64,
    pub label: Option<String>,
}

fn min_limit(a: u64, b: u64) -> u64 {
    match (a, b) {
        (0, x) | (x, 0) => x,
        (a, b) => a.min(b),
    }
}

/// Combines enabled rules with the base limits from settings.
pub fn evaluate(
    rules: &[ScheduleRule],
    base_down: u64,
    base_up: u64,
    now: NaiveDateTime,
) -> Effective {
    let mut eff = Effective {
        down: base_down,
        up: base_up,
        ..Default::default()
    };
    let enabled: Vec<_> = rules.iter().filter(|r| r.enabled).collect();
    let has_window_only = enabled
        .iter()
        .any(|r| r.action == ScheduleAction::DownloadOnly);
    let mut in_window = false;
    for r in &enabled {
        if !r.is_active(now) {
            continue;
        }
        match r.action {
            ScheduleAction::Limit { down, up } => {
                eff.down = min_limit(eff.down, down);
                eff.up = min_limit(eff.up, up);
                eff.label.get_or_insert_with(|| r.name.clone());
            }
            ScheduleAction::Pause => {
                eff.paused = true;
                eff.label = Some(r.name.clone());
            }
            ScheduleAction::DownloadOnly => in_window = true,
        }
    }
    if has_window_only && !in_window {
        eff.paused = true;
        eff.label
            .get_or_insert_with(|| "Outside download window".into());
    }
    eff
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(d: u32, h: u32, m: u32) -> NaiveDateTime {
        // 2026-11-09 is a Monday.
        NaiveDate::from_ymd_opt(2026, 11, d)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    #[test]
    fn windows() {
        let mut r = ScheduleRule::new("work", ScheduleAction::Limit { down: 100, up: 0 });
        assert!(r.is_active(at(9, 10, 0)));
        assert!(!r.is_active(at(9, 17, 0)));
        r.days = [false, true, true, true, true, true, true]; // not Monday
        assert!(!r.is_active(at(9, 10, 0)));

        let mut night = ScheduleRule::new("night", ScheduleAction::DownloadOnly);
        night.start = "23:00".into();
        night.end = "06:00".into();
        night.days = [true, false, false, false, false, false, false]; // Monday night only
        assert!(night.is_active(at(9, 23, 30)));
        assert!(night.is_active(at(10, 5, 59))); // Tuesday early morning
        assert!(!night.is_active(at(10, 23, 30)));
    }

    #[test]
    fn evaluation() {
        let limit = ScheduleRule::new(
            "work",
            ScheduleAction::Limit {
                down: 100_000,
                up: 0,
            },
        );
        let e = evaluate(std::slice::from_ref(&limit), 500_000, 0, at(9, 10, 0));
        assert_eq!(
            (e.paused, e.down, e.label.as_deref()),
            (false, 100_000, Some("work"))
        );
        let e = evaluate(std::slice::from_ref(&limit), 0, 0, at(9, 20, 0));
        assert_eq!((e.paused, e.down, e.label), (false, 0, None));

        let mut night = ScheduleRule::new("night", ScheduleAction::DownloadOnly);
        night.start = "01:00".into();
        night.end = "06:00".into();
        assert!(evaluate(std::slice::from_ref(&night), 0, 0, at(9, 12, 0)).paused);
        assert!(!evaluate(std::slice::from_ref(&night), 0, 0, at(9, 2, 0)).paused);
    }
}
