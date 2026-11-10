//! App-wide actions and their default key bindings.

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
    ]
);

/// Context for bindings that only apply while the download list has focus.
pub const LIST_CONTEXT: &str = "DownloadList";

pub fn bind(cx: &mut App) {
    let l = Some(LIST_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-w", HideWindow, None),
        KeyBinding::new("cmd-n", NewDownload, None),
        KeyBinding::new("cmd-k", OpenCommandPalette, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-1", ShowAll, None),
        KeyBinding::new("cmd-2", ShowActive, None),
        KeyBinding::new("cmd-3", ShowQueued, None),
        KeyBinding::new("cmd-4", ShowPaused, None),
        KeyBinding::new("cmd-5", ShowCompleted, None),
        KeyBinding::new("cmd-6", ShowFailed, None),
        KeyBinding::new("cmd-alt-p", PauseAll, None),
        KeyBinding::new("cmd-alt-r", ResumeAll, None),
        // List-focused, Linear-style single keys.
        KeyBinding::new("cmd-v", PasteDownload, l),
        KeyBinding::new("j", SelectNext, l),
        KeyBinding::new("down", SelectNext, l),
        KeyBinding::new("k", SelectPrev, l),
        KeyBinding::new("up", SelectPrev, l),
        KeyBinding::new("x", ToggleSelect, l),
        KeyBinding::new("cmd-a", SelectAll, l),
        KeyBinding::new("space", ToggleDetails, l),
        KeyBinding::new("enter", ToggleDetails, l),
        KeyBinding::new("escape", CloseOverlay, l),
        KeyBinding::new("p", PauseResume, l),
        KeyBinding::new("r", RetrySelected, l),
        KeyBinding::new("backspace", RemoveSelected, l),
        KeyBinding::new("delete", RemoveSelected, l),
        KeyBinding::new("cmd-backspace", RemoveSelectedWithFiles, l),
        KeyBinding::new("cmd-o", RevealSelected, l),
        KeyBinding::new("cmd-shift-o", OpenSelected, l),
        KeyBinding::new("cmd-shift-c", CopySourceUrl, l),
        KeyBinding::new("/", FocusSearch, l),
        KeyBinding::new("cmd-f", FocusSearch, None),
        KeyBinding::new("alt-up", MoveUp, l),
        KeyBinding::new("alt-down", MoveDown, l),
    ]);
}
