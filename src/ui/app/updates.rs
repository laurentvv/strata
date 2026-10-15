//! The optional check for a newer release (off by default, see `crate::update`): it runs once at start-up on a
//! worker thread and, when a newer release exists, a banner leads to its page. Nothing is downloaded or installed.

use super::*;
use crate::update::{self, Outcome, Release};
use crossbeam_channel::{Receiver, TryRecvError};

#[derive(Default)]
pub(super) struct Updates {
    started: bool,
    pub rx: Option<Receiver<Result<Outcome, String>>>,
    /// A newer release to offer.
    pub offer: Option<Release>,
}

impl App {
    /// Once per run, when the user turned the check on and it is due: ask GitHub on a worker thread.
    pub(super) fn maybe_check_for_update(&mut self) {
        if self.updates.started {
            return;
        }
        self.updates.started = true;
        // Tests and screenshots (no persistence) never touch the network.
        if !self.settings.check_updates || !self.persist_enabled || !update::due(chrono::Utc::now().timestamp(), self.settings.last_update_check) {
            return;
        }
        let (tx, rx) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = tx.send(update::latest(update::REPO, env!("CARGO_PKG_VERSION")));
        });
        self.updates.rx = Some(rx);
    }

    /// Collect the answer. A failure (offline, rate limit) is silent: the check is simply tried again at the next launch.
    pub(super) fn poll_update(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.updates.rx else { return };
        match rx.try_recv() {
            Ok(result) => {
                self.updates.rx = None;
                if let Ok(outcome) = result {
                    self.settings.last_update_check = chrono::Utc::now().timestamp();
                    if let Outcome::Available(release) = outcome {
                        if release.version != self.settings.skip_update_version {
                            self.updates.offer = Some(release);
                        }
                    }
                }
            }
            Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(500)),
            Err(TryRecvError::Disconnected) => self.updates.rx = None,
        }
    }

    /// A newer release exists: say so, and lead to its page.
    pub(super) fn update_banner(&mut self, ui: &mut egui::Ui) {
        let Some(release) = self.updates.offer.clone() else { return };
        let (mut open, mut skip, mut dismiss) = (false, false, false);
        egui::Frame::new().fill(ui.visuals().faint_bg_color).inner_margin(6.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let text = tr("A newer version is available: {version} (you have {current}).").replace("{version}", &release.version).replace("{current}", env!("CARGO_PKG_VERSION"));
            ui.label(RichText::new(text).strong());
            ui.horizontal(|ui| {
                open = ui.button(tr("Open the download page")).clicked();
                skip = ui.button(tr("Skip this version")).clicked();
                dismiss = ui.button(tr("Dismiss")).clicked();
            });
        });
        ui.add_space(4.0);
        if open {
            crate::shell::open(&PathBuf::from(&release.url));
            self.updates.offer = None;
        }
        if skip {
            self.settings.skip_update_version = release.version;
            self.updates.offer = None;
        }
        if dismiss {
            self.updates.offer = None;
        }
    }
}
