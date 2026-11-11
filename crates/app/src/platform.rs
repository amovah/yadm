//! Per-OS differences: shortcut modifiers and labels, file manager naming,
//! bundled resource locations.

use std::path::PathBuf;

pub const IS_MAC: bool = cfg!(target_os = "macos");

/// Whether YADM has a menu bar / system tray icon. On Linux the tray library
/// needs a GTK event loop that gpui doesn't run, so there is none there.
pub const HAS_TRAY: bool = cfg!(any(target_os = "macos", windows));

/// Rewrites a `cmd-` keystroke for the host OS (`ctrl-` off macOS).
///
/// gpui's static `KeyBinding::new` can't resolve `secondary-`, so bindings
/// are spelled the macOS way and translated once at startup.
pub fn keys(binding: &'static str) -> &'static str {
    if IS_MAC || !binding.contains("cmd") {
        return binding;
    }
    Box::leak(binding.replace("cmd", "ctrl").into_boxed_str())
}

/// Turns a macOS shortcut label (`⌥⌘P`, `⌘ V`, `⌘↵`) into the host OS
/// convention (`Ctrl+Alt+P`, `Ctrl+V`, `Ctrl+Enter`).
pub fn shortcut(label: &str) -> String {
    if IS_MAC {
        return label.to_string();
    }
    let (mut ctrl, mut alt, mut shift) = (false, false, false);
    let mut key = String::new();
    for c in label.chars() {
        match c {
            '⌘' | '⌃' => ctrl = true,
            '⌥' => alt = true,
            '⇧' => shift = true,
            '↵' => key.push_str("Enter"),
            '⌫' => key.push_str("Backspace"),
            ' ' => {}
            c => key.push(c),
        }
    }
    let mut out = String::new();
    for (on, name) in [(ctrl, "Ctrl+"), (alt, "Alt+"), (shift, "Shift+")] {
        if on {
            out.push_str(name);
        }
    }
    out + &key
}

/// Interface scale the desktop asks for beyond what the windowing system
/// already applies. macOS and Windows scale apps themselves (1.0). On
/// Wayland gpui only sees the monitor scale and misses GNOME's "Large text" /
/// text-scaling-factor, which GTK apps honour; under X11 gpui already reads
/// it through Xft.dpi.
pub fn system_ui_scale() -> f32 {
    static SCALE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *SCALE.get_or_init(|| {
        if !cfg!(target_os = "linux") || std::env::var_os("WAYLAND_DISPLAY").is_none_or(|v| v.is_empty()) {
            return 1.0;
        }
        std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "text-scaling-factor"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f32>().ok())
            .filter(|s| (0.5..=3.0).contains(s))
            .unwrap_or(1.0)
    })
}

/// "Show in Finder" and friends.
pub const REVEAL: &str = if cfg!(target_os = "macos") {
    "Show in Finder"
} else if cfg!(windows) {
    "Show in Explorer"
} else {
    "Show in Files"
};

/// The system trash, as users know it.
pub const TRASH: &str = if cfg!(windows) { "Recycle Bin" } else { "Trash" };

/// Whether OS notifications can be posted. macOS only allows it from a
/// signed `.app` bundle; other platforms don't need one.
pub fn system_notifications() -> bool {
    if IS_MAC {
        std::env::current_exe().is_ok_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS"))
    } else {
        true
    }
}

/// Where the unpacked browser extension lives: inside the `.app` bundle on
/// macOS, next to the executable on Windows/Linux, else the repo checkout.
pub fn extension_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let bundled = if IS_MAC { dir.parent().map(|p| p.join("Resources/extension")) } else { Some(dir.join("extension")) };
        if let Some(p) = bundled
            && p.exists()
        {
            return p;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extension")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        if IS_MAC {
            assert_eq!(shortcut("⌥⌘P"), "⌥⌘P");
            assert_eq!(keys("cmd-alt-p"), "cmd-alt-p");
        } else {
            assert_eq!(shortcut("⌥⌘P"), "Ctrl+Alt+P");
            assert_eq!(shortcut("⌘ V"), "Ctrl+V");
            assert_eq!(shortcut("⌘↵"), "Ctrl+Enter");
            assert_eq!(keys("cmd-shift-o"), "ctrl-shift-o");
            assert_eq!(keys("j"), "j");
        }
    }
}
