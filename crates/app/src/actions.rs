//! App-wide actions and their default key bindings.

use crate::platform::keys;
use gpui_kit::{App, KeyBinding};

gpui_kit::actions!(
    yadm,
    [
        Quit,
        HideWindow,
        NewDownload,
        PasteDownload,
        OpenCommandPalette,
        OpenSettings,
        ToggleDetails,
        OpenDetails,
        CloseOverlay,
        SelectNext,
        SelectPrev,
        ToggleSelect,
        SelectAll,
        PauseResume,
        RemoveSelected,
        RemoveSelectedWithFiles,
        RetrySelected,
        RevealSelected,
        OpenSelected,
        CopySourceUrl,
        PauseAll,
        ResumeAll,
        ClearFinished,
        FocusSearch,
        ShowAll,
        ShowActive,
        ShowQueued,
        ShowPaused,
        ShowCompleted,
        ShowFailed,
        CycleTheme,
        OpenDownloadsFolder,
        MoveUp,
        MoveDown,
        OpenBandwidthSettings,
    ]
);

/// Sets the global download speed limit (bytes/s, 0 = unlimited).
#[derive(Clone, PartialEq, Debug, serde::Deserialize, schemars::JsonSchema, gpui_kit::Action)]
#[action(namespace = yadm)]
pub struct SetDownloadLimit {
    pub bytes: u64,
}

/// Context for bindings that only apply while the download list has focus.
pub const LIST_CONTEXT: &str = "DownloadList";

pub fn bind(cx: &mut App) {
    let l = Some(LIST_CONTEXT);
    cx.bind_keys([
        KeyBinding::new(keys("cmd-q"), Quit, None),
        KeyBinding::new(keys("cmd-w"), HideWindow, None),
        KeyBinding::new(keys("cmd-n"), NewDownload, None),
        KeyBinding::new(keys("cmd-k"), OpenCommandPalette, None),
        KeyBinding::new(keys("cmd-,"), OpenSettings, None),
        KeyBinding::new(keys("cmd-1"), ShowAll, None),
        KeyBinding::new(keys("cmd-2"), ShowActive, None),
        KeyBinding::new(keys("cmd-3"), ShowQueued, None),
        KeyBinding::new(keys("cmd-4"), ShowPaused, None),
        KeyBinding::new(keys("cmd-5"), ShowCompleted, None),
        KeyBinding::new(keys("cmd-6"), ShowFailed, None),
        KeyBinding::new(keys("cmd-alt-p"), PauseAll, None),
        KeyBinding::new(keys("cmd-alt-r"), ResumeAll, None),
        // List-focused, Linear-style single keys.
        KeyBinding::new(keys("cmd-v"), PasteDownload, l),
        KeyBinding::new("j", SelectNext, l),
        KeyBinding::new("down", SelectNext, l),
        KeyBinding::new("k", SelectPrev, l),
        KeyBinding::new("up", SelectPrev, l),
        KeyBinding::new("x", ToggleSelect, l),
        KeyBinding::new(keys("cmd-a"), SelectAll, l),
        KeyBinding::new("space", ToggleDetails, l),
        KeyBinding::new("enter", ToggleDetails, l),
        KeyBinding::new("escape", CloseOverlay, l),
        KeyBinding::new("escape", CloseOverlay, Some("Settings")),
        KeyBinding::new("p", PauseResume, l),
        KeyBinding::new("r", RetrySelected, l),
        KeyBinding::new("backspace", RemoveSelected, l),
        KeyBinding::new("delete", RemoveSelected, l),
        KeyBinding::new(keys("cmd-backspace"), RemoveSelectedWithFiles, l),
        KeyBinding::new(keys("cmd-o"), RevealSelected, l),
        KeyBinding::new(keys("cmd-shift-o"), OpenSelected, l),
        KeyBinding::new(keys("cmd-shift-c"), CopySourceUrl, l),
        KeyBinding::new("/", FocusSearch, l),
        KeyBinding::new(keys("cmd-f"), FocusSearch, None),
        KeyBinding::new("alt-up", MoveUp, l),
        KeyBinding::new("alt-down", MoveDown, l),
    ]);
}
