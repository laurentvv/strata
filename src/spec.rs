//! Commands the ribbon and the keyboard can trigger, and the keyboard shortcut table. The ribbon is
//! laid out in `ui/app/ribbon.rs`.

use crate::settings::{SortKey, Units, ViewKind};
use eframe::egui::{Key, KeyboardShortcut, Modifiers};

/// Everything the ribbon and the keyboard can trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    ParentFolder,
    ScanDir,
    Stop,
    Refresh,
    OpenLocation,
    SortBy(SortKey),
    SetUnits(Units),
    ToggleDetails,
    Exit,
    NewInstance,
    Filter,
    Options,
    About,
    ExpandLevel(u8),
    ExpandFull,
    ExcludeSelected,
    SetView(ViewKind),
    ToggleFast,
    RescanSelected,
    ToggleMap,
    ToggleHideEmpty,
    ClearFilter,
    Find,
    /// Dock the proportional map on the right (true) or under the tree (false).
    MapRight(bool),
    SnapshotSave,
    SnapshotCompare,
    NewFolder,
    RestartAdmin,
}

const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };
const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: true };
const ALT: Modifiers = Modifiers { alt: true, ctrl: false, shift: false, mac_cmd: false, command: false };

/// Every global keyboard shortcut (the ribbon tooltips name the main ones).
pub fn shortcuts() -> Vec<(KeyboardShortcut, Cmd)> {
    let sc = KeyboardShortcut::new;
    vec![
        (sc(Modifiers::NONE, Key::F5), Cmd::Refresh),
        (sc(CTRL_SHIFT, Key::B), Cmd::Stop),
        (sc(CTRL_SHIFT, Key::N), Cmd::NewInstance),
        (sc(CTRL, Key::O), Cmd::ScanDir),
        (sc(CTRL, Key::Num1), Cmd::SortBy(SortKey::Size)),
        (sc(CTRL, Key::Num2), Cmd::SortBy(SortKey::Alloc)),
        (sc(CTRL, Key::Num3), Cmd::SortBy(SortKey::Percent)),
        (sc(CTRL, Key::Num4), Cmd::SortBy(SortKey::Files)),
        (sc(CTRL, Key::E), Cmd::ExpandFull),
        (sc(ALT, Key::ArrowUp), Cmd::ParentFolder),
        (sc(CTRL_SHIFT, Key::X), Cmd::ExcludeSelected),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_are_unique() {
        let sc = shortcuts();
        for (i, (a, _)) in sc.iter().enumerate() {
            assert!(sc.iter().skip(i + 1).all(|(b, _)| a != b), "duplicate shortcut {a:?}");
        }
    }
}
