//! The Recycle Bin: its size and an "Empty" action, so that deleting to the Recycle Bin can end with the space
//! really freed. Emptying cannot be undone, so it is always confirmed and says that it empties the whole bin.

use super::*;
use crossbeam_channel::{Receiver, TryRecvError};

#[derive(Default)]
pub(super) struct Bin {
    /// (bytes, items) of the whole Recycle Bin, once known.
    pub info: Option<(u64, u64)>,
    rx: Option<Receiver<Option<(u64, u64)>>>,
    /// The size was asked for at least once (a failing query is not repeated every frame).
    pub queried: bool,
    /// The confirmation window is open.
    pub confirm: bool,
    emptying: Option<Receiver<Result<(), String>>>,
    /// (items, bytes) just moved to the Recycle Bin by a deletion: offered to empty it.
    pub banner: Option<(usize, u64)>,
}

impl App {
    /// Ask for the size of the Recycle Bin (on a worker thread: it visits every drive).
    pub(super) fn refresh_bin(&mut self) {
        if !cfg!(windows) || self.bin.rx.is_some() {
            return;
        }
        self.bin.queried = true;
        let (tx, rx) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = tx.send(crate::trash::recycle_bin_info());
        });
        self.bin.rx = Some(rx);
    }

    /// Collect the answers of the size query and of an emptying in progress.
    pub(super) fn poll_bin(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.bin.rx {
            match rx.try_recv() {
                Ok(info) => {
                    self.bin.info = info;
                    self.bin.rx = None;
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(100)),
                Err(TryRecvError::Disconnected) => self.bin.rx = None,
            }
        }
        if let Some(rx) = &self.bin.emptying {
            match rx.try_recv() {
                Ok(result) => {
                    self.bin.emptying = None;
                    self.bin.banner = None;
                    self.notice = match result {
                        Ok(()) => tr("Recycle Bin emptied"),
                        Err(e) => format!("{}: {e}", tr("The Recycle Bin could not be emptied")),
                    };
                    self.refresh_bin();
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(100)),
                Err(TryRecvError::Disconnected) => self.bin.emptying = None,
            }
        }
    }

    /// The "Empty the Recycle Bin" button with its size; nothing when the size is not known.
    pub(super) fn bin_button(&mut self, ui: &mut egui::Ui) {
        let Some((bytes, items)) = self.bin.info else { return };
        let busy = self.bin.emptying.is_some() || self.delete_job.is_some();
        let label = format!("{} ({})", tr("Empty the Recycle Bin"), format_bytes(bytes));
        if ui.add_enabled(items > 0 && !busy, egui::Button::new(label)).on_hover_text(tr("Free the space of what was moved to the Recycle Bin.")).clicked() {
            self.bin.confirm = true;
        }
    }

    /// After a deletion to the Recycle Bin: the space is not free yet, and here is how to free it.
    pub(super) fn bin_banner(&mut self, ui: &mut egui::Ui) {
        let Some((items, bytes)) = self.bin.banner else { return };
        let mut dismiss = false;
        egui::Frame::new().fill(ui.visuals().faint_bg_color).inner_margin(6.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(format!("{}: {items} - {}", tr("Moved to the Recycle Bin"), format_bytes(bytes))).strong());
            ui.label(tr("The space is only freed once the Recycle Bin is emptied."));
            ui.horizontal(|ui| {
                self.bin_button(ui);
                dismiss = ui.button(tr("Dismiss")).clicked();
            });
        });
        ui.add_space(4.0);
        if dismiss {
            self.bin.banner = None;
        }
    }

    /// Confirmation of emptying the whole Recycle Bin.
    pub(super) fn bin_confirm_window(&mut self, ctx: &egui::Context) {
        if !self.bin.confirm {
            return;
        }
        let Some((bytes, items)) = self.bin.info else {
            self.bin.confirm = false;
            return;
        };
        let (mut yes, mut no) = (false, false);
        egui::Window::new(tr("Empty the Recycle Bin")).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(RichText::new(format!("{} {} - {}", fmt::group_int(items), tr("items"), format_bytes(bytes))).strong());
            ui.label(tr("This empties the whole Recycle Bin, including what was not deleted with Strata. It cannot be undone."));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                yes = ui.button(tr("Empty the Recycle Bin")).clicked();
                no = ui.button(tr("Cancel")).clicked();
            });
        });
        if no {
            self.bin.confirm = false;
        } else if yes {
            self.bin.confirm = false;
            let (tx, rx) = crossbeam_channel::bounded(1);
            std::thread::spawn(move || {
                let _ = tx.send(crate::trash::empty_recycle_bin());
            });
            self.bin.emptying = Some(rx);
            self.notice = tr("Emptying the Recycle Bin...");
        }
    }
}
