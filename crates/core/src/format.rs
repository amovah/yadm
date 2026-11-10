//! Human-readable formatting helpers shared by the UI and tray.

pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else if v >= 10.0 {
        format!("{v:.1} {}", UNITS[i])
    } else {
        format!("{v:.2} {}", UNITS[i])
    }
}

pub fn speed(bps: u64) -> String {
    format!("{}/s", bytes(bps))
}

/// Compact duration: `45s`, `3m 12s`, `2h 05m`, `3d 4h`.
pub fn duration(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Relative time like Linear: `now`, `5m`, `3h`, `2d`, `Mar 4`.
pub fn relative(ts_ms: i64, now_ms: i64) -> String {
    let secs = ((now_ms - ts_ms) / 1000).max(0);
    if secs < 60 {
        "now".into()
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else if secs < 86400 * 7 {
        format!("{}d", secs / 86400)
    } else {
        chrono::DateTime::from_timestamp_millis(ts_ms)
            .map(|d| d.with_timezone(&chrono::Local).format("%b %-d").to_string())
            .unwrap_or_default()
    }
}

/// Parses user input like `500K`, `2M`, `1.5 MB`, `0` into bytes/sec. Bare numbers are KB.
pub fn parse_rate(input: &str) -> Option<u64> {
    let s = input.trim().to_ascii_uppercase().replace(' ', "");
    let s = s.trim_end_matches("/S").trim_end_matches('B');
    if s.is_empty() {
        return None;
    }
    let (num, mult) = match s.chars().last()? {
        'K' => (&s[..s.len() - 1], 1024.0),
        'M' => (&s[..s.len() - 1], 1024.0 * 1024.0),
        'G' => (&s[..s.len() - 1], 1024.0 * 1024.0 * 1024.0),
        _ => (s, 1024.0),
    };
    let v: f64 = num.parse().ok()?;
    (v >= 0.0).then(|| (v * mult) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1536), "1.50 KB");
        assert_eq!(bytes(250 * 1024 * 1024), "250 MB");
        assert_eq!(duration(75), "1m 15s");
        assert_eq!(duration(7300), "2h 01m");
        assert_eq!(parse_rate("500K"), Some(512_000));
        assert_eq!(parse_rate("2 MB/s"), Some(2 * 1024 * 1024));
        assert_eq!(parse_rate("100"), Some(102_400));
        assert_eq!(parse_rate("abc"), None);
    }
}
