//! Options and About dialogs.

use super::{assets, App};
use crate::spec::Cmd;
use crate::i18n::{self, tr};
use crate::settings::{SizeMode, Theme, Units};
use eframe::egui::{self, RichText};
use std::path::PathBuf;

fn units_label(u: Units) -> String {
    match u {
        Units::Auto => tr("Automatic Units"),
        Units::Bytes => tr("Values in Byte"),
        Units::KB => tr("Values in KB"),
        Units::MB => tr("Values in MB"),
        Units::GB => tr("Values in GB"),
        Units::TB => tr("Values in TB"),
    }
}

/// The selectable languages (system default, English, every translation), shared by the Options
/// dialog and the ribbon.
pub(super) fn language_choices(ui: &mut egui::Ui, current: &mut String) {
    // Scroll instead of running off the screen when the list grows.
    egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
        let cur = current.clone();
        if ui.selectable_label(cur == "auto", language_label("auto")).clicked() {
            *current = "auto".into();
        }
        if ui.selectable_label(cur == "en", "English (en)").clicked() {
            *current = "en".into();
        }
        for l in i18n::LOCALES {
            if ui.selectable_label(cur.eq_ignore_ascii_case(l.code), format!("{} ({})", l.name, l.code)).clicked() {
                *current = l.code.to_string();
            }
        }
    });
}

/// Text of the language combo for the current setting.
fn language_label(setting: &str) -> String {
    if setting.eq_ignore_ascii_case("auto") || setting.is_empty() {
        return format!("{} ({})", tr("Detect automatically"), i18n::resolve_language(setting));
    }
    match i18n::find_locale(setting) {
        Some(l) => format!("{} ({})", l.name, l.code),
        None => "English (en)".to_string(),
    }
}

impl App {
    pub(super) fn options_window(&mut self, ctx: &egui::Context) {
        if !self.show_options {
            return;
        }
        fn section(ui: &mut egui::Ui, title: &str) {
            ui.add_space(10.0);
            ui.label(RichText::new(title).strong());
            ui.separator();
        }
        let mut open = true;
        let mut rescan = false;
        egui::Window::new(tr("Options")).open(&mut open).collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.set_min_width(440.0);
            ui.spacing_mut().item_spacing.y = 6.0;
            section(ui, &tr("Display"));
            egui::Grid::new("options_display").num_columns(2).spacing([16.0, 8.0]).min_col_width(110.0).show(ui, |ui| {
                ui.label(tr("Language"));
                egui::ComboBox::from_id_salt("lang").width(280.0).selected_text(language_label(&self.settings.language)).show_ui(ui, |ui| {
                    language_choices(ui, &mut self.settings.language);
                });
                ui.end_row();

                ui.label(tr("Unit"));
                egui::ComboBox::from_id_salt("units").width(280.0).selected_text(units_label(self.settings.units)).show_ui(ui, |ui| {
                    for u in Units::ALL {
                        ui.selectable_value(&mut self.settings.units, u, units_label(u));
                    }
                });
                ui.end_row();

                ui.label(tr("Decimals"));
                ui.horizontal(|ui| {
                    for n in 0..=3u8 {
                        ui.selectable_value(&mut self.settings.decimals, n, n.to_string());
                    }
                });
                ui.end_row();

                ui.label(tr("Percent"));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.settings.size_mode, SizeMode::Size, tr("Size"));
                    ui.selectable_value(&mut self.settings.size_mode, SizeMode::Allocated, tr("Allocated Space"));
                });
                ui.end_row();

                ui.label(tr("Style"));
                let theme_label = |t: Theme| match t {
                    Theme::Auto => tr("Detect automatically"),
                    Theme::Light => tr("Light"),
                    Theme::Dark => tr("Dark"),
                };
                egui::ComboBox::from_id_salt("theme").width(280.0).selected_text(theme_label(self.settings.theme)).show_ui(ui, |ui| {
                    for t in [Theme::Auto, Theme::Light, Theme::Dark] {
                        ui.selectable_value(&mut self.settings.theme, t, theme_label(t));
                    }
                });
                ui.end_row();
            });

            section(ui, &tr("Columns"));
            ui.checkbox(&mut self.settings.show_allocated, tr("Allocated"));
            ui.checkbox(&mut self.settings.details, tr("Show Details"));
            ui.checkbox(&mut self.settings.show_map, tr("Proportional map"));

            section(ui, &tr("Scan"));
            ui.checkbox(&mut self.fast_ntfs, tr("Fast NTFS scan (admin)"));
            ui.checkbox(&mut self.settings.always_admin, tr("Always start as administrator"));
            ui.checkbox(&mut self.settings.warn_not_admin, tr("Warn when not running as administrator"));
            if cfg!(windows) {
                let mut on = *self.explorer_menu.get_or_insert_with(|| crate::shell_ext::is_installed(crate::shell_ext::KEY_NAME));
                if ui.checkbox(&mut on, tr("Show in the Explorer context menu")).changed() {
                    let ok = if on {
                        std::env::current_exe().is_ok_and(|exe| crate::shell_ext::install(&exe, crate::shell_ext::KEY_NAME, &tr("Analyze with Strata")))
                    } else {
                        crate::shell_ext::uninstall(crate::shell_ext::KEY_NAME)
                    };
                    self.explorer_menu = Some(if ok { on } else { !on });
                }
            }

            section(ui, &tr("Updates"));
            ui.checkbox(&mut self.settings.check_updates, tr("Check for a newer version when Strata starts"));
            ui.label(RichText::new(tr("Asks GitHub for the latest release at most once a day (GitHub sees your IP address, as for any web request). No information about your files is sent, and nothing is downloaded or installed.")).weak());

            section(ui, &tr("Excluded folders"));
            ui.label(RichText::new(tr("These folders are left out of the results. Restore one to see it again after the next scan.")).weak());
            let mut restore = None;
            egui::ScrollArea::vertical().max_height(120.0).id_salt("excl").auto_shrink([false, true]).show(ui, |ui| {
                if self.excludes.is_empty() {
                    ui.weak(tr("(none)"));
                }
                for (i, e) in self.excludes.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button(tr("Restore")).clicked() {
                            restore = Some(i);
                        }
                        ui.add(egui::Label::new(e.display().to_string()).truncate());
                    });
                }
            });
            if let Some(i) = restore {
                self.excludes.remove(i);
                self.excludes_changed = true;
            }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.exclude_input).desired_width(300.0).hint_text(tr("Path to scan")));
                if ui.button(tr("Add new")).clicked() && !self.exclude_input.trim().is_empty() {
                    self.excludes.push(PathBuf::from(self.exclude_input.trim()));
                    self.exclude_input.clear();
                    self.excludes_changed = true;
                }
                if ui.button("...").clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        self.excludes.push(d);
                        self.excludes_changed = true;
                    }
                }
            });
            if self.excludes_changed && self.loaded.is_some() && self.handle.is_none() {
                ui.horizontal(|ui| {
                    ui.label(tr("The list changed."));
                    if ui.button(tr("Rescan now")).clicked() {
                        rescan = true;
                    }
                });
            }
            ui.add_space(8.0);
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Close")).clicked() {
                    self.show_options = false;
                }
            });
        });
        if rescan {
            self.run_cmd(ctx, Cmd::Refresh);
        }
        if !open {
            self.show_options = false;
        }
    }

    /// About box: icon, name, version and a short description.
    pub(super) fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            self.about_copied = false;
            return;
        }
        if self.about_icon.is_none() {
            self.about_icon = assets::icon_texture(ctx);
        }
        let mut open = true;
        egui::Window::new(tr("About Strata")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(tex) = &self.about_icon {
                    ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(96.0, 96.0)));
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new("Strata").size(26.0).strong());
                    ui.label(format!("{} {}", tr("Version"), env!("CARGO_PKG_VERSION")));
                    ui.label(tr("Disk space analyzer"));
                    ui.add_space(6.0);
                    ui.weak(tr("Live treemap, scan history, clean-up tools."));
                    ui.weak(tr("Written in Rust with egui."));
                    if let Some(g) = crate::diag::graphics() {
                        ui.weak(g);
                    }
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(tr("Close")).clicked() {
                    self.show_about = false;
                }
                if ui.button(tr("Copy diagnostics")).on_hover_text(tr("Copy the version, Windows build and graphics driver to paste into a bug report.")).clicked() {
                    ui.ctx().copy_text(crate::diag::diagnostics());
                    self.about_copied = true;
                }
                if self.about_copied {
                    ui.weak(tr("Copied"));
                }
            });
        });
        if !open {
            self.show_about = false;
        }
    }
}
