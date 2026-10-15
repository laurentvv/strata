//! Space that is easy to overlook because it sits in no folder of yours: the hibernation and paging files,
//! restore points, the Recycle Bin, the Windows component store and update caches. This module only finds
//! them in a scanned tree; what they are and how to reduce them is told by the view.

use crate::scan::{NodeId, Tree};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Hibernation,
    PagingFiles,
    RestorePoints,
    RecycleBin,
    ComponentStore,
    UpdateDownloads,
    InstallerCache,
    PreviousWindows,
    UpgradeLeftovers,
    DriverStore,
}

impl Item {
    pub const ALL: [Item; 10] = [
        Item::Hibernation,
        Item::PagingFiles,
        Item::RestorePoints,
        Item::RecycleBin,
        Item::ComponentStore,
        Item::UpdateDownloads,
        Item::InstallerCache,
        Item::PreviousWindows,
        Item::UpgradeLeftovers,
        Item::DriverStore,
    ];

    /// Where the item lives, relative to the root of the drive (lowercase, backslashes). Several paths add up.
    fn paths(self) -> &'static [&'static str] {
        match self {
            Item::Hibernation => &["hiberfil.sys"],
            Item::PagingFiles => &["pagefile.sys", "swapfile.sys"],
            Item::RestorePoints => &["system volume information"],
            Item::RecycleBin => &["$recycle.bin"],
            Item::ComponentStore => &["windows\\winsxs"],
            Item::UpdateDownloads => &["windows\\softwaredistribution\\download"],
            Item::InstallerCache => &["windows\\installer"],
            Item::PreviousWindows => &["windows.old"],
            Item::UpgradeLeftovers => &["$windows.~bt", "$windows.~ws"],
            Item::DriverStore => &["windows\\system32\\driverstore\\filerepository"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// Seen in the scan (its size may still be small).
    Found,
    /// Seen, but Windows refused to list its content: the size is a lower bound.
    Unreadable,
    /// Not in the scanned part of the drive.
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub item: Item,
    /// Space on disk of everything found.
    pub size: u64,
    pub presence: Presence,
}

/// The node at `path` (drive-relative, lowercase, backslashes) when the scan root lies `root_rel` below the drive
/// root (empty for a whole drive). Folder names are compared without regard to case, as Windows does.
fn locate(tree: &Tree, root_rel: &[String], path: &str) -> Option<NodeId> {
    let comps: Vec<&str> = path.split('\\').collect();
    if root_rel.len() > comps.len() || !root_rel.iter().zip(&comps).all(|(a, b)| a.eq_ignore_ascii_case(b)) {
        return None;
    }
    let mut cur = Tree::ROOT;
    for c in &comps[root_rel.len()..] {
        cur = tree.children(cur).iter().copied().find(|&k| tree.name(k).eq_ignore_ascii_case(c))?;
    }
    Some(cur)
}

/// Every known item, with what a scan of `tree` found. `denied` holds the folders that could not be read.
pub fn find(tree: &Tree, root_rel: &[String], denied: &HashSet<PathBuf>) -> Vec<Found> {
    if tree.nodes.is_empty() {
        return Vec::new();
    }
    Item::ALL
        .iter()
        .map(|&item| {
            let nodes: Vec<NodeId> = item.paths().iter().filter_map(|p| locate(tree, root_rel, p)).collect();
            let size = nodes.iter().map(|&n| tree.get(n).alloc).sum();
            let presence = if nodes.is_empty() {
                Presence::Absent
            } else if nodes.iter().any(|&n| denied.contains(&tree.path_of(n))) {
                Presence::Unreadable
            } else {
                Presence::Found
            };
            Found { item, size, presence }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::scan_simple;
    use std::path::Path;

    fn write(p: &Path, n: usize) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![1u8; n]).unwrap();
    }

    fn get(found: &[Found], item: Item) -> &Found {
        found.iter().find(|f| f.item == item).unwrap()
    }

    fn drive_like_fixture() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(&r.join("hiberfil.sys"), 4000);
        write(&r.join("pagefile.sys"), 3000);
        write(&r.join("swapfile.sys"), 100);
        write(&r.join("Windows/WINSXS/a/b.bin"), 5000);
        write(&r.join("Windows/SoftwareDistribution/Download/u.cab"), 700);
        write(&r.join("Windows/Installer/x.msi"), 400);
        write(&r.join("Windows.old/Users/f.bin"), 6000);
        write(&r.join("$WINDOWS.~BT/s.bin"), 50);
        std::fs::create_dir_all(r.join("System Volume Information")).unwrap();
        d
    }

    #[test]
    fn items_of_a_whole_drive_are_found_with_their_sizes_whatever_the_case() {
        let d = drive_like_fixture();
        let tree = scan_simple(d.path()).tree;
        let found = find(&tree, &[], &HashSet::new());
        assert_eq!(found.len(), Item::ALL.len());
        assert!(get(&found, Item::Hibernation).size >= 4000);
        assert_eq!(get(&found, Item::Hibernation).presence, Presence::Found);
        assert!(get(&found, Item::PagingFiles).size >= 3100, "pagefile and swapfile add up");
        assert!(get(&found, Item::ComponentStore).size >= 5000, "WINSXS is found as WinSxS");
        assert!(get(&found, Item::UpdateDownloads).size >= 700);
        assert!(get(&found, Item::InstallerCache).size >= 400);
        assert!(get(&found, Item::PreviousWindows).size >= 6000);
        assert!(get(&found, Item::UpgradeLeftovers).size >= 50);
        assert_eq!(get(&found, Item::RestorePoints).presence, Presence::Found, "an empty folder is still there");
        assert_eq!(get(&found, Item::DriverStore).presence, Presence::Absent);
        assert_eq!(get(&found, Item::RecycleBin).presence, Presence::Absent);
        assert_eq!(get(&found, Item::DriverStore).size, 0);
    }

    #[test]
    fn a_folder_windows_refused_to_list_is_reported_as_unreadable() {
        let d = drive_like_fixture();
        let tree = scan_simple(d.path()).tree;
        let svi = tree.children(Tree::ROOT).iter().copied().find(|&k| tree.name(k) == "System Volume Information").unwrap();
        let denied: HashSet<PathBuf> = [tree.path_of(svi)].into_iter().collect();
        let found = find(&tree, &[], &denied);
        assert_eq!(get(&found, Item::RestorePoints).presence, Presence::Unreadable);
        assert_eq!(get(&found, Item::Hibernation).presence, Presence::Found, "the others are unaffected");
    }

    #[test]
    fn scanning_only_the_windows_folder_finds_what_is_inside_it_and_nothing_outside() {
        let d = drive_like_fixture();
        let tree = scan_simple(&d.path().join("Windows")).tree;
        let found = find(&tree, &["Windows".to_string()], &HashSet::new());
        assert!(get(&found, Item::ComponentStore).size >= 5000);
        assert_eq!(get(&found, Item::Hibernation).presence, Presence::Absent, "outside the scanned folder");
        assert_eq!(get(&found, Item::PreviousWindows).presence, Presence::Absent);
    }

    #[test]
    fn a_scan_that_starts_deeper_than_an_item_does_not_match_it() {
        let d = drive_like_fixture();
        let tree = scan_simple(&d.path().join("Windows/Installer")).tree;
        let rel = ["Windows".to_string(), "Installer".to_string()];
        let found = find(&tree, &rel, &HashSet::new());
        assert!(get(&found, Item::InstallerCache).size >= 400, "the scan root is the item itself");
        assert_eq!(get(&found, Item::ComponentStore).presence, Presence::Absent);
    }
}
