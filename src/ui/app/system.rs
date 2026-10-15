//! The "System space" view: Windows' own big files and folders that are easy to overlook (hibernation and paging
//! files, restore points, the component store...), with what each one is and how to reduce it. Strata deletes
//! nothing here: the commands are copied for the user to run in a window opened as administrator.

use super::*;
use crate::analysis::system_space::{self, Found, Item, Presence};
use std::path::{Component, Path, Prefix};

struct Text {
    name: &'static str,
    what: &'static str,
    how: &'static str,
}

fn text(item: Item) -> Text {
    match item {
        Item::Hibernation => Text {
            name: "Hibernation file (hiberfil.sys)",
            what: "Holds the memory content when the PC hibernates, and is used by fast start-up. Its size depends on the amount of RAM.",
            how: "Turn hibernation off to delete it, or keep a smaller file that only supports fast start-up. Run these in a Command Prompt or PowerShell opened as administrator.",
        },
        Item::PagingFiles => Text {
            name: "Paging files (pagefile.sys, swapfile.sys)",
            what: "Windows' overflow memory on disk. It is also where a crash dump is written.",
            how: "Best left alone: Windows sizes these files itself, and without a page file no crash dump can be written.",
        },
        Item::RestorePoints => Text {
            name: "Restore points and shadow copies",
            what: "System Volume Information holds restore points and shadow copies, the history Windows keeps to roll back. Only an administrator can read it, so Strata often cannot size it.",
            how: "Limit the space it may use (System Properties > System Protection > Configure). Lowering the limit can delete existing restore points. Run these as administrator.",
        },
        Item::RecycleBin => Text {
            name: "Recycle Bin",
            what: "Files you deleted, kept until the bin is emptied (all drives together).",
            how: "Empty it from here: this cannot be undone.",
        },
        Item::ComponentStore => Text {
            name: "Windows component store (WinSxS)",
            what: "The files Windows needs to repair and update itself. File Explorer and Strata overstate its size, because many files are hard links shared with other Windows folders.",
            how: "Never delete it by hand. The first command shows its real size; the second removes superseded updates. Run these as administrator.",
        },
        Item::UpdateDownloads => Text {
            name: "Windows Update downloads",
            what: "Windows Update's download cache.",
            how: "Windows Disk Cleanup removes what is no longer needed (Clean up system files > Windows Update Cleanup).",
        },
        Item::InstallerCache => Text {
            name: "Installer cache (Windows\\Installer)",
            what: "The installation packages Windows keeps so programs can be repaired, updated and uninstalled.",
            how: "Leave it: removing files from it breaks repair and uninstall of programs.",
        },
        Item::PreviousWindows => Text {
            name: "Previous Windows installation (Windows.old)",
            what: "Your previous Windows installation, kept so you can roll back after an upgrade.",
            how: "Windows Disk Cleanup (Previous Windows installation(s)) or Settings > System > Storage > Temporary files remove it; after that you cannot roll back. Strata's Clean-up view lists it too.",
        },
        Item::UpgradeLeftovers => Text {
            name: "Windows upgrade leftovers ($WINDOWS.~BT)",
            what: "Temporary files of a Windows upgrade.",
            how: "Windows Disk Cleanup (Clean up system files) lists them when they can go.",
        },
        Item::DriverStore => Text {
            name: "Driver store",
            what: "Copies of the drivers Windows can install.",
            how: "Leave it, unless you know which old drivers to remove: the command lists them.",
        },
    }
}

/// Commands worth copying for an item (all run as administrator).
fn commands(item: Item, drive: &str) -> Vec<String> {
    match item {
        Item::Hibernation => vec!["powercfg /hibernate off".into(), "powercfg /hibernate /type reduced".into()],
        Item::RestorePoints => vec!["vssadmin list shadowstorage".into(), format!("vssadmin resize shadowstorage /for={drive}: /on={drive}: /maxsize=10GB")],
        Item::ComponentStore => vec!["Dism.exe /Online /Cleanup-Image /AnalyzeComponentStore".into(), "Dism.exe /Online /Cleanup-Image /StartComponentCleanup".into()],
        Item::DriverStore => vec!["pnputil /enum-drivers".into()],
        _ => Vec::new(),
    }
}

/// The drive letter and the folders between the drive root and the scan root (empty for a whole drive).
fn drive_and_rel(root: &Path) -> (String, Vec<String>) {
    let mut drive = "C".to_string();
    let mut rel = Vec::new();
    for c in root.components() {
        match c {
            Component::Prefix(p) => {
                if let Prefix::Disk(b) | Prefix::VerbatimDisk(b) = p.kind() {
                    drive = (b as char).to_ascii_uppercase().to_string();
                }
            }
            Component::Normal(n) => rel.push(n.to_string_lossy().into_owned()),
            Component::RootDir | Component::CurDir | Component::ParentDir => {}
        }
    }
    (drive, rel)
}

/// One line of the view: the item, its size (None when Windows refused to list it) and whether it was in the scan.
struct Entry {
    item: Item,
    size: Option<u64>,
    present: bool,
}

impl App {
    /// Windows' own big files and folders that are easy to overlook, with how to reduce them.
    pub(super) fn system_view(&mut self, ui: &mut egui::Ui) {
        let Some(l) = &self.loaded else {
            ui.label(tr("Scan a folder first."));
            return;
        };
        if l.tree.live.is_some() {
            ui.label(tr("Available when the scan has finished."));
            return;
        }
        let (drive, rel) = drive_and_rel(&l.root);
        let whole_drive = rel.is_empty();
        let found_alloc = l.tree.get(Tree::ROOT).alloc;
        let drive_used = if whole_drive { scan::drive_used_bytes(&l.root) } else { None };
        if self.system_cache.is_none() {
            let denied: HashSet<PathBuf> = l.error_list.iter().map(|e| e.path.clone()).collect();
            self.system_cache = Some(system_space::find(&l.tree, &rel, &denied));
        }
        if self.bin.info.is_none() && !self.bin.queried {
            self.refresh_bin();
        }
        let found: Vec<Found> = self.system_cache.clone().unwrap_or_default();
        // The Recycle Bin is read from Windows (the folder itself is closed to a normal user and lives on every drive).
        let mut entries: Vec<Entry> = found
            .iter()
            .filter_map(|f| match (f.item, self.bin.info) {
                (Item::RecycleBin, Some((bytes, _))) => (bytes > 0).then_some(Entry { item: f.item, size: Some(bytes), present: true }),
                _ => Some(Entry { item: f.item, size: (f.presence != Presence::Unreadable).then_some(f.size), present: f.presence != Presence::Absent }),
            })
            .collect();
        // What could not be sized comes first (it may be big), then the biggest.
        entries.sort_by(|a, b| a.size.is_some().cmp(&b.size.is_some()).then(b.size.unwrap_or(0).cmp(&a.size.unwrap_or(0))));
        let l10n = fmt::loc();
        let mut copied: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(tr("Windows system files that are easy to overlook, and how to reduce them. Strata deletes nothing here.")).weak());
            if !whole_drive {
                ui.label(RichText::new(tr("Scan a whole drive (for example C:\\) to see everything; only what lies inside the scanned folder is listed.")).color(super::super::theme::RISK_CHECK));
            }
            if let Some(used) = drive_used.filter(|&u| u > found_alloc) {
                let line = tr("The drive reports {used} in use; Strata found {found}; about {gap} was not seen.")
                    .replace("{used}", &fmt::format_bytes_short_in(used, l10n))
                    .replace("{found}", &fmt::format_bytes_short_in(found_alloc, l10n))
                    .replace("{gap}", &fmt::format_bytes_short_in(used - found_alloc, l10n));
                ui.label(RichText::new(line).strong());
                ui.label(RichText::new(tr("Not seen: the file system's own bookkeeping, restore points and shadow copies, and anything that could not be read.")).weak());
            } else if let Some(used) = drive_used.filter(|&u| found_alloc > u + u / 100) {
                let line = tr("Strata counted {found}, more than the {used} the drive reports: files with several names (hard links, mostly in WinSxS) are probably counted once per name.")
                    .replace("{found}", &fmt::format_bytes_short_in(found_alloc, l10n))
                    .replace("{used}", &fmt::format_bytes_short_in(used, l10n));
                ui.label(RichText::new(line).weak());
            }
            ui.add_space(6.0);
            for e in entries.iter().filter(|e| e.present) {
                let t = text(e.item);
                egui::Frame::new().fill(ui.visuals().faint_bg_color).inner_margin(8.0).corner_radius(4.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr(t.name)).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match e.size {
                            Some(s) => {
                                ui.label(RichText::new(format_bytes(s)).strong());
                            }
                            None => {
                                ui.label(RichText::new(tr("Not readable without administrator rights")).color(super::super::theme::RISK_CHECK));
                            }
                        });
                    });
                    ui.label(tr(t.what));
                    ui.label(RichText::new(tr(t.how)).weak());
                    for cmd in commands(e.item, &drive) {
                        ui.horizontal(|ui| {
                            ui.code(&cmd);
                            if ui.small_button(tr("Copy")).clicked() {
                                ui.ctx().copy_text(cmd.clone());
                                copied = Some(cmd.clone());
                            }
                            if self.system_copied.as_deref() == Some(cmd.as_str()) {
                                ui.weak(tr("Copied"));
                            }
                        });
                    }
                    match e.item {
                        Item::RecycleBin => {
                            ui.horizontal(|ui| self.bin_button(ui));
                        }
                        Item::UpdateDownloads | Item::PreviousWindows | Item::UpgradeLeftovers if cfg!(windows) && ui.button(tr("Windows Disk Cleanup")).clicked() => {
                            let _ = std::process::Command::new("cleanmgr.exe").spawn();
                        }
                        _ => {}
                    }
                });
                ui.add_space(6.0);
            }
            let absent: Vec<String> = entries.iter().filter(|e| !e.present).map(|e| tr(text(e.item).name)).collect();
            if !absent.is_empty() {
                ui.weak(format!("{} {}", tr("Not found in this scan:"), absent.join(", ")));
            }
        });
        if copied.is_some() {
            self.system_copied = copied;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_and_the_view_strings_have_a_french_translation() {
        let mut all: Vec<&str> = Item::ALL.iter().flat_map(|&i| {
            let t = text(i);
            [t.name, t.what, t.how]
        }).collect();
        all.extend([
            "System space",
            "Windows system files that are easy to overlook, and how to reduce them. Strata deletes nothing here.",
            "Scan a whole drive (for example C:\\) to see everything; only what lies inside the scanned folder is listed.",
            "The drive reports {used} in use; Strata found {found}; about {gap} was not seen.",
            "Not seen: the file system's own bookkeeping, restore points and shadow copies, and anything that could not be read.",
            "Strata counted {found}, more than the {used} the drive reports: files with several names (hard links, mostly in WinSxS) are probably counted once per name.",
            "Not readable without administrator rights",
            "Not found in this scan:",
            "Copy",
            "What is this?",
            "See what takes space outside your folders.",
        ]);
        let missing: Vec<&&str> = all.iter().filter(|t| !crate::i18n::is_translated("fr", t)).collect();
        assert!(missing.is_empty(), "no French for: {missing:#?}");
    }

    #[test]
    fn commands_are_given_only_where_they_help_and_name_the_scanned_drive() {
        assert_eq!(commands(Item::Hibernation, "D")[0], "powercfg /hibernate off");
        assert!(commands(Item::RestorePoints, "D").iter().any(|c| c == "vssadmin resize shadowstorage /for=D: /on=D: /maxsize=10GB"));
        assert!(commands(Item::ComponentStore, "C").iter().any(|c| c.contains("/StartComponentCleanup")));
        assert!(commands(Item::PagingFiles, "C").is_empty(), "the paging files are best left alone");
        assert!(commands(Item::InstallerCache, "C").is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn the_drive_and_the_way_down_to_the_scan_root_are_read_from_its_path() {
        assert_eq!(drive_and_rel(Path::new(r"C:\")), ("C".to_string(), Vec::<String>::new()));
        assert_eq!(drive_and_rel(Path::new(r"d:\Windows")), ("D".to_string(), vec!["Windows".to_string()]));
        assert_eq!(drive_and_rel(Path::new(r"\\?\E:\a\b")), ("E".to_string(), vec!["a".to_string(), "b".to_string()]));
    }
}
