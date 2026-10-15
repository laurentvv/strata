//! Deleting files and folders: one confirmation window (Recycle Bin by default, permanent deletion on
//! request) and a background job with progress, shared by the tree, the lists, the clean-up view and the
//! duplicate finder.

use super::*;
use std::sync::Arc;

/// One thing to delete. `node` is its place in the loaded tree when it has one (duplicates are plain paths).
#[derive(Clone)]
pub(super) struct DelItem {
    pub path: PathBuf,
    pub size: u64,
    pub node: Option<NodeId>,
}

pub(super) struct DeleteRequest {
    pub items: Vec<DelItem>,
}

enum Msg {
    Done { index: usize, result: Result<(), String> },
    Finished,
}

/// A deletion running on a worker thread.
pub(super) struct DeleteJob {
    rx: crossbeam_channel::Receiver<Msg>,
    cancel: Arc<AtomicBool>,
    items: Vec<DelItem>,
    permanently: bool,
    done: usize,
    freed: u64,
    removed: HashSet<PathBuf>,
    failed: Vec<String>,
}

impl App {
    /// Ask for confirmation before deleting nodes of the loaded tree (nested picks are merged).
    pub(super) fn ask_delete_nodes(&mut self, ids: Vec<NodeId>) {
        let Some(l) = &self.loaded else { return };
        let ids = prune_nested(&l.tree, ids.into_iter().filter(|&i| i != Tree::ROOT && (i as usize) < l.tree.nodes.len()).collect());
        let items: Vec<DelItem> = ids.into_iter().map(|id| DelItem { path: l.tree.path_of(id), size: l.tree.get(id).size, node: Some(id) }).collect();
        self.ask_delete(items);
    }

    /// Ask for confirmation before deleting plain files (duplicate finder).
    pub(super) fn ask_delete_files(&mut self, files: Vec<(PathBuf, u64)>) {
        self.ask_delete(files.into_iter().map(|(path, size)| DelItem { path, size, node: None }).collect());
    }

    fn ask_delete(&mut self, items: Vec<DelItem>) {
        if items.is_empty() || self.delete_job.is_some() {
            return;
        }
        self.delete_permanently = false; // the safe choice is the default every time
        self.confirm = Some(DeleteRequest { items });
    }

    /// The confirmation window, the Delete key, the error report and the progress window.
    pub(super) fn delete_window(&mut self, ctx: &egui::Context) {
        if self.confirm.is_none() && self.delete_error.is_none() && self.delete_job.is_none() {
            // Only where the selection is visible: a stale selection from another view must not be deletable blind.
            let selection_visible = matches!(self.view, ViewKind::Tree | ViewKind::Largest | ViewKind::Oldest | ViewKind::Cleanup);
            if selection_visible && self.handle.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Delete)) && !ctx.egui_wants_keyboard_input() {
                if self.multi.len() > 1 {
                    let ids: Vec<NodeId> = self.multi.iter().copied().collect();
                    self.ask_delete_nodes(ids);
                } else if let Some(id) = self.selected {
                    self.ask_delete_nodes(vec![id]);
                }
            }
            return;
        }
        if let Some(err) = self.delete_error.clone() {
            let mut open = true;
            egui::Window::new(tr("Delete")).open(&mut open).collapsible(false).show(ctx, |ui| {
                ui.label(err);
                if ui.button(tr("OK")).clicked() {
                    self.delete_error = None;
                }
            });
            if !open {
                self.delete_error = None;
            }
            return;
        }
        if self.delete_job.is_some() {
            self.delete_progress_window(ctx);
            return;
        }
        let Some(req) = &self.confirm else { return };
        let total: u64 = req.items.iter().map(|i| i.size).sum();
        let count = req.items.len();
        let mut permanently = self.delete_permanently;
        let title = tr(if permanently { "Delete permanently" } else { "Delete (Recycle Bin)" });
        let (mut yes, mut no) = (false, false);
        egui::Window::new(title).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(RichText::new(format!("{} {} - {}", count, tr("items selected"), format_bytes(total))).strong());
            egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                for item in req.items.iter().take(200) {
                    ui.add(egui::Label::new(item.path.display().to_string()).truncate());
                }
                if count > 200 {
                    ui.weak(format!("... +{}", count - 200));
                }
            });
            ui.add_space(4.0);
            ui.checkbox(&mut permanently, tr("Delete permanently (skip the Recycle Bin)"));
            if permanently {
                ui.label(RichText::new(tr("This cannot be undone. The space is freed immediately.")).color(super::super::theme::RISK_CHECK));
            } else {
                ui.label(RichText::new(tr("Moved to the Recycle Bin: the space is only freed once it is emptied.")).weak());
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                yes = ui.button(tr("Delete")).clicked();
                no = ui.button(tr("Cancel")).clicked();
            });
        });
        self.delete_permanently = permanently;
        if no {
            self.confirm = None;
        } else if yes {
            if let Some(req) = self.confirm.take() {
                self.start_delete(req.items, permanently);
            }
        }
    }

    fn start_delete(&mut self, items: Vec<DelItem>, permanently: bool) {
        let (tx, rx) = crossbeam_channel::unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let (work, stop) = (items.clone(), cancel.clone());
        std::thread::spawn(move || {
            for (index, item) in work.iter().enumerate() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let result = if permanently { crate::trash::delete_permanently(&item.path) } else { crate::trash::recycle(&item.path) };
                let _ = tx.send(Msg::Done { index, result });
            }
            let _ = tx.send(Msg::Finished);
        });
        self.delete_job = Some(DeleteJob { rx, cancel, items, permanently, done: 0, freed: 0, removed: HashSet::new(), failed: Vec::new() });
    }

    /// Collect the results of the running deletion; refresh the views when it ends.
    pub(super) fn poll_delete(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.delete_job else { return };
        let mut finished = false;
        while let Ok(msg) = job.rx.try_recv() {
            match msg {
                Msg::Done { index, result } => {
                    job.done += 1;
                    let item = &job.items[index];
                    match result {
                        Ok(()) => {
                            job.freed += item.size;
                            job.removed.insert(item.path.clone());
                            if let (Some(id), Some(l)) = (item.node, &mut self.loaded) {
                                l.tree.remove_subtree(id);
                            }
                        }
                        Err(e) => job.failed.push(format!("{}: {e}", item.path.display())),
                    }
                }
                Msg::Finished => finished = true,
            }
        }
        if !finished {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        let Some(job) = self.delete_job.take() else { return };
        if let Some(groups) = &mut self.dup_result {
            duplicates::remove_paths(groups, &job.removed);
        }
        self.selected = None;
        self.multi.clear();
        self.cleanup_checked.clear();
        self.view_root = Tree::ROOT;
        self.invalidate();
        let verb = if job.permanently { tr("Deleted") } else { tr("Moved to the Recycle Bin") };
        self.notice = format!("{verb}: {} - {}", job.removed.len(), format_bytes(job.freed));
        if !job.failed.is_empty() {
            self.delete_error = Some(job.failed.join("\n"));
        }
    }

    fn delete_progress_window(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.delete_job else { return };
        let (done, total, freed) = (job.done, job.items.len(), job.freed);
        let mut stop = false;
        egui::Window::new(tr("Deleting...")).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                scan_marker(ui, 14.0);
                ui.label(format!("{done} / {total} - {}", format_bytes(freed)));
            });
            if ui.button(tr("Stop")).clicked() {
                stop = true;
            }
        });
        if stop {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}
