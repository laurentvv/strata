//! Declarative description of the menu bar: captions, hints, shortcuts and the command each entry
//! triggers. The ribbon is laid out in `ui/app/ribbon.rs`; this table feeds the File-tab menu bar
//! and the global keyboard shortcuts.

use crate::settings::{SortKey, Units, ViewKind};
use eframe::egui::{Key, KeyboardShortcut, Modifiers};

/// Everything the menu bar and the toolbar can trigger.
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

/// One entry of the menu bar.
#[derive(Debug, Clone, Default)]
pub struct Item {
    /// Menu caption in English (`&` marks the accelerator); translated through `i18n::tr`.
    pub caption: &'static str,
    /// Tooltip in English; empty for none.
    pub hint: &'static str,
    pub cmd: Option<Cmd>,
    pub shortcut: Option<KeyboardShortcut>,
    pub separator: bool,
    pub children: Vec<Item>,
}

const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };
const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: true };
const ALT: Modifiers = Modifiers { alt: true, ctrl: false, shift: false, mac_cmd: false, command: false };

fn leaf(caption: &'static str, hint: &'static str, cmd: Cmd, shortcut: Option<KeyboardShortcut>) -> Item {
    Item { caption, hint, cmd: Some(cmd), shortcut, ..Item::default() }
}

fn sep() -> Item {
    Item { separator: true, ..Item::default() }
}

fn menu(caption: &'static str, children: Vec<Item>) -> Item {
    Item { caption, children, ..Item::default() }
}

fn key(mods: Modifiers, k: Key) -> Option<KeyboardShortcut> {
    Some(KeyboardShortcut::new(mods, k))
}

/// The menu bar: File, Scan, View, Expand, Options, Help.
pub fn menu_items() -> Vec<Item> {
    vec![
        menu(
            "&File",
            vec![
                leaf("&Refresh", "Scan the current folder again", Cmd::Refresh, key(Modifiers::NONE, Key::F5)),
                leaf("S&top scan", "Stop the running scan and keep what was found", Cmd::Stop, key(CTRL_SHIFT, Key::B)),
                leaf("&New window", "Open another Strata window", Cmd::NewInstance, key(CTRL_SHIFT, Key::N)),
                sep(),
                leaf("E&xit", "Close Strata", Cmd::Exit, None),
            ],
        ),
        menu("&Scan", vec![leaf("Choose &folder to scan", "Pick the folder or drive to analyze", Cmd::ScanDir, key(CTRL, Key::O))]),
        menu(
            "&View",
            vec![
                leaf("&Size", "Show and sort by the size of files and folders", Cmd::SortBy(SortKey::Size), key(CTRL, Key::Num1)),
                leaf("&Allocated space", "Show and sort by the space used on disk", Cmd::SortBy(SortKey::Alloc), key(CTRL, Key::Num2)),
                leaf("&Percent", "Show how much of its parent each item takes", Cmd::SortBy(SortKey::Percent), key(CTRL, Key::Num3)),
                leaf("&File count", "Show and sort by the number of files", Cmd::SortBy(SortKey::Files), key(CTRL, Key::Num4)),
                sep(),
                leaf("Sizes in &bytes", "", Cmd::SetUnits(Units::Bytes), None),
                leaf("Sizes in &KB", "", Cmd::SetUnits(Units::KB), None),
                leaf("Sizes in &MB", "", Cmd::SetUnits(Units::MB), None),
                leaf("Sizes in &GB", "", Cmd::SetUnits(Units::GB), None),
                leaf("Sizes in &TB", "", Cmd::SetUnits(Units::TB), None),
                leaf("&Automatic units", "Pick the unit that fits each size", Cmd::SetUnits(Units::Auto), None),
                sep(),
                leaf("Show d&etails", "Show the Files, Folders and Last modified columns", Cmd::ToggleDetails, None),
            ],
        ),
        menu(
            "&Expand",
            vec![
                leaf("To level &1", "", Cmd::ExpandLevel(1), None),
                leaf("To level &2", "", Cmd::ExpandLevel(2), None),
                leaf("To level &3", "", Cmd::ExpandLevel(3), None),
                leaf("To level &4", "", Cmd::ExpandLevel(4), None),
                leaf("To level &5", "", Cmd::ExpandLevel(5), None),
                leaf("To level &6", "", Cmd::ExpandLevel(6), None),
                sep(),
                leaf("&Expand all", "Show every folder of the tree", Cmd::ExpandFull, key(CTRL, Key::E)),
            ],
        ),
        menu("&Options", vec![leaf("&Filter", "Show only files and folders matching a filter", Cmd::Filter, None), leaf("&Settings", "", Cmd::Options, None)]),
        menu("&Help", vec![leaf("&About Strata", "", Cmd::About, None)]),
    ]
}

/// Every shortcut of the menu table plus the ones that have no menu entry.
pub fn shortcuts() -> Vec<(KeyboardShortcut, Cmd)> {
    fn walk(items: &[Item], out: &mut Vec<(KeyboardShortcut, Cmd)>) {
        for it in items {
            if let (Some(sc), Some(cmd)) = (it.shortcut, it.cmd) {
                out.push((sc, cmd));
            }
            walk(&it.children, out);
        }
    }
    let mut out = Vec::new();
    walk(&menu_items(), &mut out);
    out.push((KeyboardShortcut::new(ALT, Key::ArrowUp), Cmd::ParentFolder));
    out.push((KeyboardShortcut::new(CTRL_SHIFT, Key::X), Cmd::ExcludeSelected));
    out
}

/// "Ctrl+Shift+B" style text of a shortcut.
pub fn shortcut_text(sc: &KeyboardShortcut) -> String {
    sc.format(&eframe::egui::ModifierNames::NAMES, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(items: &[Item], out: &mut Vec<Item>) {
        for i in items {
            out.push(i.clone());
            flat(&i.children, out);
        }
    }

    #[test]
    fn shortcuts_are_unique_and_every_leaf_has_a_command() {
        let mut all = Vec::new();
        flat(&menu_items(), &mut all);
        assert!(all.iter().all(|i| i.separator || !i.children.is_empty() || i.cmd.is_some()), "a leaf without command");
        let sc = shortcuts();
        for (i, (a, _)) in sc.iter().enumerate() {
            assert!(sc.iter().skip(i + 1).all(|(b, _)| a != b), "duplicate shortcut {}", shortcut_text(a));
        }
    }

    #[test]
    fn captions_have_one_accelerator_at_most() {
        let mut all = Vec::new();
        flat(&menu_items(), &mut all);
        for i in all.iter().filter(|i| !i.separator) {
            assert!(i.caption.matches('&').count() <= 1, "{}", i.caption);
        }
    }
}
