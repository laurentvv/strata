//! Compact tabbed ribbon (Home / Analyse / View / Options / Help); the icons are vector
//! glyphs from `paint`.

use super::*;

/// Ribbon caption: the English text translated, without accelerator.
fn cap(_id: &str, english: &str) -> String {
    tr(english)
}

/// Split a button caption on a space so that it fits in two centred lines.
pub(super) fn split_caption(ui: &egui::Ui, text: &str, font: &egui::FontId, max_w: f32) -> Vec<String> {
    let width = |t: &str| ui.painter().layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE).size().x;
    if width(text) <= max_w {
        return vec![text.to_string()];
    }
    let mut best: Option<(f32, usize)> = None;
    for (i, ch) in text.char_indices() {
        if ch == ' ' {
            let w = width(&text[..i]).max(width(&text[i + 1..]));
            if best.is_none_or(|(bw, _)| w < bw) {
                best = Some((w, i));
            }
        }
    }
    match best {
        Some((_, i)) => vec![text[..i].to_string(), text[i + 1..].to_string()],
        // A single long word is never cut in the middle: the button gets wider instead.
        None => vec![text.to_string()],
    }
}

/// One-sentence explanation shown when hovering a ribbon button, keyed by its English caption
/// (accelerator marks removed). Every value has a French entry (a test checks it).
pub(super) const HINTS: &[(&str, &str)] = &[
    ("Select Directory", "Choose the folder or drive to analyze (Ctrl+O)."),
    ("Recent folders", "Scan again one of the folders analyzed recently."),
    ("Stop Scan", "Stop the running scan and keep what was found."),
    ("Refresh", "Scan the current folder again (F5)."),
    ("New window", "Open another Strata window (Ctrl+Shift+N)."),
    ("Size", "Sort by the size of files and folders (Ctrl+1)."),
    ("Allocated Space", "Sort by the space really used on disk (Ctrl+2)."),
    ("File Count", "Sort by the number of files (Ctrl+4)."),
    ("Percent", "Sort by the share each item takes of its parent (Ctrl+3)."),
    ("Automatic Units", "Pick the unit (KB, MB, GB...) that fits each size."),
    ("Parent Folder", "Go up one level: select the parent of the selected folder (in the map and chart: show the parent). At the top of the tree, scan the folder above (Alt+Up)."),
    ("Open Location", "Show the selected item in Windows Explorer."),
    ("Exclude", "Hide the selected folder from the results. It is listed under Options > Excluded folders, where you can bring it back (Ctrl+Shift+X)."),
    ("Rescan Folder", "Scan only the selected folder again and update its totals."),
    ("Expand", "Open the tree down to a chosen level."),
    ("New folder", "Create a folder inside the selected folder."),
    ("Restart as Administrator", "Relaunch Strata with administrator rights to read protected folders."),
    ("Export", "Save the results as CSV, HTML or JSON."),
    ("Tree", "Folders and files as an expandable table."),
    ("Treemap", "Full-size proportional map of the selected folder."),
    ("Chart", "Pie chart of the biggest items of the selected folder."),
    ("File types", "Space used by each file extension and category."),
    ("Top 100 files", "The 100 biggest files."),
    ("Oldest files", "The files that were not modified for the longest time."),
    ("Duplicates", "Find identical files and see what deleting the extra copies would free."),
    ("Clean-up", "Folders that can usually be deleted safely (build output, caches...) and empty folders."),
    ("File age", "How much space was last modified in each age range."),
    ("Filter", "Show only files matching a name, size, date or type."),
    ("Clear filter", "Remove the filter and show everything again."),
    ("Hide empty folders", "Do not list folders that hold no data."),
    ("Find in tree", "Jump to the next item whose name contains some text (Ctrl+F)."),
    ("Changes", "What grew or vanished since an earlier scan of this folder."),
    ("Compare...", "Compare with a snapshot file saved earlier."),
    ("Snapshot...", "Save the current results as a snapshot file."),
    ("Fast NTFS (admin)", "Read the NTFS file table directly: much faster on a whole drive, needs administrator rights."),
    ("Show Details", "Show the Files, Folders and Last Modified columns."),
    ("Configure columns", "Choose which columns the tree shows."),
    ("Decimals", "Number of decimals in sizes."),
    ("Proportional map", "Show or hide the map under or beside the tree."),
    ("Expand all", "Open every folder of the tree (Ctrl+E)."),
    ("Map at the bottom", "Dock the map under the tree."),
    ("Map on the right", "Dock the map beside the tree."),
    ("Level of detail", "How many folder levels the map draws."),
    ("Map colors", "Colour files by type or by folder level."),
    ("Level 1", "Open the tree down to level 1."),
    ("Level 2", "Open the tree down to level 2."),
    ("Level 3", "Open the tree down to level 3."),
    ("Language", "Change the language of Strata."),
    ("Style", "Light or dark appearance."),
    ("Options", "All settings, including the excluded folders."),
    ("About", "Version and information about Strata."),
];

/// Hover text of the ribbon button with this English caption (empty when unknown).
fn hint_for(english: &str) -> String {
    let key = crate::i18n::strip_accel(english);
    match HINTS.iter().find(|(k, _)| *k == key) {
        Some((_, h)) => tr(h),
        None => {
            #[cfg(test)]
            MISSING_HINTS.with(|m| m.borrow_mut().push(key));
            String::new()
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Captions drawn without an explanation (filled while the UI tests draw every ribbon tab).
    pub(super) static MISSING_HINTS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

const CAPTION_FONT: f32 = 11.0;
const MIN_BUTTON_W: f32 = 58.0;

/// Big icon button with a caption underneath. `caption` is the English text: it is translated here
/// and also selects the hover explanation. The button is as wide as its widest caption line.
fn big_button(ui: &mut egui::Ui, ic: Icon, caption: &str, enabled: bool, checked: bool) -> egui::Response {
    let label = tr(caption);
    let font = egui::FontId::proportional(CAPTION_FONT);
    let lines = split_caption(ui, &label, &font, MIN_BUTTON_W - 2.0);
    let text_w = lines.iter().map(|l| ui.painter().layout_no_wrap(l.clone(), font.clone(), Color32::WHITE).size().x).fold(0.0, f32::max);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2((text_w + 10.0).max(MIN_BUTTON_W), 64.0), if enabled { Sense::click() } else { Sense::hover() });
    let resp = resp.on_hover_text(hint_for(caption));
    let resp = if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
    let vis = ui.visuals();
    if checked {
        ui.painter().rect_filled(rect, 2.0, vis.selection.bg_fill);
    } else if resp.hovered() && enabled {
        ui.painter().rect_filled(rect, 2.0, vis.widgets.hovered.weak_bg_fill);
    }
    let col = if enabled { vis.text_color() } else { vis.weak_text_color() };
    paint::icon(ui.painter(), egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + 19.0), egui::vec2(28.0, 28.0)), ic, enabled);
    for (i, line) in lines.iter().take(2).enumerate() {
        ui.painter().text(egui::pos2(rect.center().x, rect.top() + 37.0 + i as f32 * 12.5), egui::Align2::CENTER_TOP, line, font.clone(), col);
    }
    resp
}

/// A named group: content on top, the group caption centred underneath, a separator after it.
fn group(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        let inner = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            content(ui);
        });
        let w = inner.response.rect.width().max(40.0);
        ui.allocate_ui_with_layout(egui::vec2(w, 14.0), egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.label(RichText::new(title).small().weak());
        });
    });
    ui.separator();
}

impl App {
    /// Ribbon button for a command; `caption` is the English text (translated by `big_button`).
    fn big(&self, ui: &mut egui::Ui, out: &mut Option<Cmd>, cmd: Cmd, ic: Icon, caption: &str) {
        if big_button(ui, ic, caption, self.cmd_enabled(cmd), self.cmd_checked(cmd)).clicked() {
            *out = Some(cmd);
        }
    }

    pub(super) fn ribbon(&mut self, ui: &mut egui::Ui) -> Option<Cmd> {
        let mut out: Option<Cmd> = None;
        let names = [
            cap("TabHomeTitleAction", "&Home"),
            cap("ScanTabTitleAction", "&Scan"),
            cap("TabViewTitleAction", "&View"),
            cap("TabOptionsTitleAction", "&Options"),
            cap("HelpTabTitleAction", "Hel&p"),
        ];
        let accent = super::super::theme::TAB_ACCENT;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, n) in names.iter().enumerate() {
                let active = self.ribbon_tab == i;
                let resp = ui.add(egui::Label::new(RichText::new(n)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
                let r = resp.rect.expand2(egui::vec2(8.0, 3.0));
                if active {
                    ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(2.0, accent));
                }
                if resp.clicked() {
                    self.ribbon_tab = i;
                }
                ui.add_space(10.0);
            }
        });
        ui.add_space(2.0);
        ui.separator();
        let scanning = self.handle.is_some();
        // Wide tabs scroll sideways instead of being cut off at the window edge.
        egui::ScrollArea::horizontal().id_salt("ribbon_scroll").auto_shrink([false, true]).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.set_min_height(88.0);
            ui.spacing_mut().item_spacing.x = 2.0;
            match self.ribbon_tab {
                0 => {
                    group(ui, &cap("ScanTabTitleAction", "&Scan"), |ui| {
                        self.big(ui, &mut out, Cmd::ScanDir, Icon::Folder, "Select &Directory");
                        self.big(ui, &mut out, Cmd::Stop, Icon::Stop, "Stop Scan");
                        self.big(ui, &mut out, Cmd::Refresh, Icon::Refresh, "Refresh");
                    });
                    group(ui, &cap("GrpModeTitleAction", "Mode"), |ui| {
                        self.big(ui, &mut out, Cmd::SortBy(SortCol::Size), Icon::Bars, "Size");
                        self.big(ui, &mut out, Cmd::SortBy(SortCol::Alloc), Icon::Drive, "Allocated Space");
                        self.big(ui, &mut out, Cmd::SortBy(SortCol::Files), Icon::Files, "File Count");
                        self.big(ui, &mut out, Cmd::SortBy(SortCol::Percent), Icon::Percent, "Percent");
                    });
                    group(ui, &cap("GrpUnitTitleAction", "Unit"), |ui| {
                        self.big(ui, &mut out, Cmd::SetUnits(Units::Auto), Icon::Units, "Automatic Units");
                        ui.vertical(|ui| {
                            for (u, l) in [(Units::TB, "TB"), (Units::GB, "GB"), (Units::MB, "MB"), (Units::KB, "KB"), (Units::Bytes, "B")] {
                                if ui.add(egui::Button::new(RichText::new(l).small().strong()).selected(self.settings.units == u).min_size(egui::vec2(30.0, 18.0))).clicked() {
                                    out = Some(Cmd::SetUnits(u));
                                }
                            }
                        });
                    });
                    group(ui, &cap("GrpFolderOperationsAction", "Folder Operations"), |ui| {
                        self.big(ui, &mut out, Cmd::ParentFolder, Icon::Up, "Parent Folder");
                        self.big(ui, &mut out, Cmd::OpenLocation, Icon::Open, "Open Location");
                        self.big(ui, &mut out, Cmd::ExcludeSelected, Icon::Exclude, "Exclude");
                        self.big(ui, &mut out, Cmd::RescanSelected, Icon::Refresh, "Rescan Folder");
                        let resp = big_button(ui, Icon::Expand, "Expand", self.loaded.is_some(), false);
                        egui::Popup::menu(&resp).show(|ui| {
                            for n in 1..=3u8 {
                                if ui.button(format!("{} {n}", tr("Level"))).clicked() {
                                    out = Some(Cmd::ExpandLevel(n));
                                    ui.close();
                                }
                            }
                            if ui.button(tr("Expand all")).clicked() {
                                out = Some(Cmd::ExpandFull);
                                ui.close();
                            }
                        });
                        self.big(ui, &mut out, Cmd::NewFolder, Icon::Folder, "New folder");
                    });
                    group(ui, &cap("GrpToolsTitleAction", "Tools"), |ui| {
                        self.big(ui, &mut out, Cmd::RestartAdmin, Icon::Bolt, "Restart as Administrator");
                        self.big(ui, &mut out, Cmd::NewInstance, Icon::Files, "New window");
                        let resp = big_button(ui, Icon::Export, "Export", self.loaded.is_some() && !scanning, false);
                        egui::Popup::menu(&resp).show(|ui| {
                            for (label, fmt) in [("CSV...", export::Format::Csv), ("HTML report...", export::Format::Html), ("JSON...", export::Format::Json)] {
                                if ui.button(tr(label)).clicked() {
                                    self.export(fmt);
                                    ui.close();
                                }
                            }
                        });
                    });
                }
                1 => {
                    group(ui, &tr("Views"), |ui| {
                        for (v, ic, l) in [
                            (ViewKind::Tree, Icon::Tree, "Tree"),
                            (ViewKind::Treemap, Icon::Treemap, "Treemap"),
                            (ViewKind::Chart, Icon::Chart, "Chart"),
                            (ViewKind::Types, Icon::Types, "File types"),
                            (ViewKind::Largest, Icon::Top, "Top 100 files"),
                            (ViewKind::Oldest, Icon::Oldest, "Oldest files"),
                            (ViewKind::Duplicates, Icon::Dupes, "Duplicates"),
                            (ViewKind::Cleanup, Icon::Exclude, "Clean-up"),
                            (ViewKind::Age, Icon::Oldest, "File age"),
                        ] {
                            self.big(ui, &mut out, Cmd::SetView(v), ic, l);
                        }
                    });
                    group(ui, &tr("Filter"), |ui| {
                        self.big(ui, &mut out, Cmd::Filter, Icon::Filter, "Filter");
                        self.big(ui, &mut out, Cmd::ClearFilter, Icon::Exclude, "Clear filter");
                        self.big(ui, &mut out, Cmd::ToggleHideEmpty, Icon::Folder, "Hide empty folders");
                        self.big(ui, &mut out, Cmd::Find, Icon::Types, "Find in tree");
                    });
                    group(ui, &tr("History"), |ui| {
                        self.big(ui, &mut out, Cmd::SetView(ViewKind::Compare), Icon::Oldest, "Changes");
                        self.big(ui, &mut out, Cmd::SnapshotCompare, Icon::Open, "Compare...");
                        self.big(ui, &mut out, Cmd::SnapshotSave, Icon::Export, "Snapshot...");
                    });
                    group(ui, &tr("Scan"), |ui| {
                        if big_button(ui, Icon::Bolt, "Fast NTFS (admin)", !scanning, self.fast_ntfs).clicked() {
                            out = Some(Cmd::ToggleFast);
                        }
                    });
                }
                2 => {
                    group(ui, &cap("GrpViewTitleAction", "View"), |ui| {
                        self.big(ui, &mut out, Cmd::ToggleDetails, Icon::Details, "Show Details");
                        let resp = big_button(ui, Icon::Details, "Configure columns", true, false);
                        egui::Popup::menu(&resp).show(|ui| {
                            for (c, label) in [(SortCol::Size, "Size"), (SortCol::Alloc, "Allocated"), (SortCol::Percent, "% of Parent"), (SortCol::Files, "Files"), (SortCol::Folders, "Folders"), (SortCol::Modified, "Last Modified")] {
                                let mut on = self.column_visible(c);
                                if ui.checkbox(&mut on, tr(label)).clicked() {
                                    self.toggle_column(c);
                                }
                            }
                        });
                        let resp = big_button(ui, Icon::Percent, "Decimals", true, false);
                        egui::Popup::menu(&resp).show(|ui| {
                            for n in 0..=3u8 {
                                if ui.selectable_label(self.settings.decimals == n, n.to_string()).clicked() {
                                    self.settings.decimals = n;
                                    ui.close();
                                }
                            }
                        });
                        self.big(ui, &mut out, Cmd::ToggleMap, Icon::Treemap, "Proportional map");
                        self.big(ui, &mut out, Cmd::ExpandFull, Icon::Expand, "Expand all");
                    });
                    group(ui, &tr("Proportional map"), |ui| {
                        self.big(ui, &mut out, Cmd::MapRight(false), Icon::Treemap, "Map at the bottom");
                        self.big(ui, &mut out, Cmd::MapRight(true), Icon::Treemap, "Map on the right");
                        let resp = big_button(ui, Icon::Tree, "Level of detail", self.settings.show_map, false);
                        egui::Popup::menu(&resp).show(|ui| {
                            for n in 1..=4u8 {
                                if ui.selectable_label(self.settings.map_levels == n, format!("{} {n}", tr("Level"))).clicked() {
                                    self.settings.map_levels = n;
                                    ui.close();
                                }
                            }
                        });
                        let colors = big_button(ui, Icon::Types, "Map colors", self.settings.show_map, false);
                        egui::Popup::menu(&colors).show(|ui| {
                            for (by_type, label) in [(true, "By file type"), (false, "By folder level")] {
                                if ui.selectable_label(self.settings.map_by_type == by_type, tr(label)).clicked() {
                                    self.settings.map_by_type = by_type;
                                    ui.close();
                                }
                            }
                        });
                        ui.vertical(|ui| {
                            ui.checkbox(&mut self.settings.map_unique_files, tr("Show unique files"));
                            ui.checkbox(&mut self.settings.map_headers, tr("Show hierarchy"));
                            ui.checkbox(&mut self.settings.map_legend, tr("Show legend"));
                            ui.checkbox(&mut self.settings.map_labels, tr("Show labels"));
                            ui.checkbox(&mut self.settings.map_free, tr("Show free space on the drive"));
                        });
                    });
                    group(ui, &tr("Expand"), |ui| {
                        for n in 1..=3u8 {
                            self.big(ui, &mut out, Cmd::ExpandLevel(n), Icon::Tree, &format!("Level {n}"));
                        }
                    });
                }
                3 => group(ui, &tr("Options"), |ui| {
                    let resp = big_button(ui, Icon::Types, "Language", true, false);
                    egui::Popup::menu(&resp).show(|ui| {
                        super::dialogs::language_choices(ui, &mut self.settings.language);
                    });
                    let resp = big_button(ui, Icon::Chart, "Style", true, false);
                    egui::Popup::menu(&resp).show(|ui| {
                        ui.selectable_value(&mut self.settings.theme, Theme::Auto, tr("Detect automatically"));
                        ui.selectable_value(&mut self.settings.theme, Theme::Light, tr("Light"));
                        ui.selectable_value(&mut self.settings.theme, Theme::Dark, tr("Dark"));
                    });
                    self.big(ui, &mut out, Cmd::Options, Icon::Options, "Options");
                }),
                _ => group(ui, &cap("GrpHelpTitleAction", "Help"), |ui| {
                    self.big(ui, &mut out, Cmd::About, Icon::About, "About");
                }),
            }
        });
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_caption_and_hint_has_a_french_text() {
        let missing: Vec<&str> = HINTS.iter().flat_map(|(k, h)| [*k, *h]).filter(|t| !crate::i18n::is_translated("fr", t)).collect();
        assert!(missing.is_empty(), "no French for: {missing:#?}");
    }

    #[test]
    fn hints_are_unique_and_end_with_a_full_stop() {
        for (i, (k, h)) in HINTS.iter().enumerate() {
            assert!(h.ends_with('.'), "{k}: {h}");
            assert!(HINTS.iter().skip(i + 1).all(|(k2, _)| k != k2), "duplicate hint for {k}");
        }
    }
}
