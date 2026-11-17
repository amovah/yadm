//! Start YADM when the user logs in, through each OS's free per-user
//! mechanism: a LaunchAgent plist on macOS, the `Run` registry key on
//! Windows and an XDG autostart entry on Linux. Launches carry
//! [`HIDDEN_ARG`] so YADM starts in the background.

use anyhow::Result;

/// Passed by the login entry: start without opening the main window.
pub const HIDDEN_ARG: &str = "--hidden";

/// Writes (`true`) or removes (`false`) the login entry. The entry points at
/// the running binary and is only rewritten when that changed.
pub fn set(enabled: bool) -> Result<()> {
    if enabled { imp::install() } else { imp::remove() }
}

/// Brings the OS entry in line with the setting at startup: follows the
/// binary if it moved, or clears a leftover entry.
pub fn sync(enabled: bool) {
    if let Err(e) = set(enabled) {
        tracing::warn!("could not update the login item: {e:#}");
    }
}

/// Writes `bytes` to `path` unless it already holds exactly that.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn write_if_changed(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn remove_file(path: &std::path::Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use anyhow::Context as _;
    use std::path::{Path, PathBuf};

    const LABEL: &str = "dev.yadm.app";

    fn path() -> Result<PathBuf> {
        let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).context("HOME is not set")?;
        Ok(PathBuf::from(home).join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
    }

    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
    }

    /// Loaded by launchd at the next login; not `launchctl load`ed now, which
    /// would start a second copy of the running app.
    pub(super) fn plist(exe: &Path) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>{HIDDEN_ARG}</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
  <key>LimitLoadToSessionType</key><string>Aqua</string>
</dict>
</plist>
"#,
            xml_escape(&exe.to_string_lossy())
        )
    }

    pub fn install() -> Result<()> {
        let exe = std::env::current_exe()?;
        // A quarantined app opened where it was unzipped runs from a random
        // read-only copy that is gone after a reboot.
        anyhow::ensure!(
            !exe.to_string_lossy().contains("/AppTranslocation/"),
            "move YADM.app to Applications and open it from there first"
        );
        write_if_changed(&path()?, plist(&exe).as_bytes())
    }

    pub fn remove() -> Result<()> {
        remove_file(&path()?)
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use anyhow::Context as _;
    use std::path::{Path, PathBuf};

    fn path() -> Result<PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").filter(|h| !h.is_empty()).map(|h| PathBuf::from(h).join(".config")))
            .context("neither XDG_CONFIG_HOME nor HOME is set")?;
        Ok(config.join("autostart/dev.yadm.app.desktop"))
    }

    pub(super) fn desktop_entry(exe: &Path) -> String {
        let exec = exe.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
        format!(
            "[Desktop Entry]\nType=Application\nName=YADM\nComment=Start YADM in the background at login\n\
             Exec=\"{exec}\" {HIDDEN_ARG}\nIcon=dev.yadm.app\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
        )
    }

    pub fn install() -> Result<()> {
        let exe = std::env::current_exe()?;
        write_if_changed(&path()?, desktop_entry(&exe).as_bytes())
    }

    pub fn remove() -> Result<()> {
        remove_file(&path()?)
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// Also removed by the uninstaller (scripts/windows/yadm.iss).
    const VALUE: &str = "YADM";

    pub fn install() -> Result<()> {
        let exe = crate::platform::native_path(&std::env::current_exe()?);
        let command = format!("\"{}\" {HIDDEN_ARG}", exe.display());
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN)?;
        if key.get_value::<String, _>(VALUE).is_ok_and(|old| old == command) {
            return Ok(());
        }
        key.set_value(VALUE, &command)?;
        Ok(())
    }

    pub fn remove() -> Result<()> {
        let key = match RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN, KEY_READ | KEY_SET_VALUE) {
            Ok(key) => key,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        match key.delete_value(VALUE) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod imp {
    use super::*;

    pub fn install() -> Result<()> {
        Ok(())
    }

    pub fn remove() -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    #[test]
    fn plist_escapes_path() {
        let p = super::imp::plist(std::path::Path::new("/Apps/A & B/YADM.app/Contents/MacOS/yadm"));
        assert!(p.contains("<string>/Apps/A &amp; B/YADM.app/Contents/MacOS/yadm</string>"));
        assert!(p.contains("<string>--hidden</string>"));
        assert!(p.contains("<key>RunAtLoad</key><true/>"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_entry_quotes_path() {
        let e = super::imp::desktop_entry(std::path::Path::new("/opt/my apps/\"y\"/yadm"));
        assert!(e.contains("Exec=\"/opt/my apps/\\\"y\\\"/yadm\" --hidden\n"));
    }
}
