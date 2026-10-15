//! Windowless UI tests. The real `App::draw` runs inside an `egui_kittest` harness and is driven with
//! pointer and keyboard events at the screen position of real widgets, so they cover what only manual
//! clicks used to catch: row selection from every column, right-click, Enter in the name dialogs,
//! keyboard navigation and the clickable lists.
//!
//! Nothing is written to the user's data: persistence is switched off and every scan targets a
//! temporary folder.

use super::*;
use egui::{Event, Key, PointerButton, Pos2};
use egui_kittest::{kittest::Queryable, Harness};
use std::path::Path;

/// The UI language, units and decimals are process-wide: UI tests run one at a time.
fn lock() -> std::sync::MutexGuard<'static, ()> {
    crate::i18n::LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

/// root/d1 .. d8, each holding one file `f.bin` of a distinct size.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    for i in 1..=8u32 {
        let d = root.join(format!("d{i}"));
        std::fs::create_dir(&d).unwrap();
        std::fs::write(d.join("f.bin"), vec![1u8; (i * 3000) as usize]).unwrap();
    }
    Fixture { _dir: dir, root }
}

fn harness(root: &Path) -> Harness<'static, App> {
    let ctx = egui::Context::default();
    let st = Settings { language: "en".into(), ..Settings::default() };
    let mut app = App::new(&ctx, st, &RunOptions::default());
    app.persist_enabled = false;
    app.settings_path = None;
    app.path_text = root.to_string_lossy().into_owned();
    app.start_scan();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(0.05).build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    for _ in 0..600 {
        h.step();
        if h.state().handle.is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(h.state().handle.is_none(), "the scan did not finish");
    h.run_steps(4);
    h
}

fn click_at(h: &mut Harness<'_, App>, pos: Pos2, button: PointerButton) {
    h.event(Event::PointerMoved(pos));
    h.run_steps(2);
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE });
    }
    h.run_steps(3);
}

fn center_of(h: &Harness<'_, App>, label: &str) -> Pos2 {
    h.get_by_label(label).rect().center()
}

/// x of the column header `caption` that sits above `below_y` (the ribbon has buttons with the
/// same texts higher up, so the nearest match above the row wins).
fn header_x(h: &Harness<'_, App>, caption: &str, below_y: f32) -> f32 {
    h.get_all_by_label(caption)
        .filter(|n| n.rect().center().y < below_y)
        .max_by(|a, b| a.rect().center().y.total_cmp(&b.rect().center().y))
        .unwrap_or_else(|| panic!("no header '{caption}'"))
        .rect()
        .center()
        .x
}

/// A context-menu entry: the lowest match on screen (the ribbon repeats some captions at the top).
fn menu_item_pos(h: &Harness<'_, App>, label: &str) -> Pos2 {
    h.get_all_by_label(label)
        .max_by(|a, b| a.rect().center().y.total_cmp(&b.rect().center().y))
        .unwrap_or_else(|| panic!("no menu entry '{label}'"))
        .rect()
        .center()
}

fn node_named(h: &Harness<'_, App>, name: &str) -> NodeId {
    h.state().loaded.as_ref().unwrap().tree.find_child(Tree::ROOT, name).unwrap()
}

/// Right-click the row of folder `name` and choose `entry` in the menu.
fn row_menu(h: &mut Harness<'_, App>, name: &str, entry: &str) {
    let p = center_of(h, name);
    click_at(h, p, PointerButton::Secondary);
    let item = menu_item_pos(h, entry);
    click_at(h, item, PointerButton::Primary);
}

#[test]
fn a_click_in_any_column_selects_the_row() {
    let _g = lock();
    let fx = fixture();
    let mut h = harness(&fx.root);
    let cols = h.state().tree_columns();
    assert!(cols.len() >= 6, "{cols:?}");
    for (i, &col) in cols.iter().enumerate() {
        let name = format!("d{}", i % 8 + 1);
        let id = node_named(&h, &name);
        h.state_mut().selected = None;
        h.run_steps(2);
        let name_pos = center_of(&h, &name);
        let x = if col == SortCol::Name { name_pos.x } else { header_x(&h, &h.state().column_caption(col), name_pos.y) };
        click_at(&mut h, Pos2::new(x, name_pos.y), PointerButton::Primary);
        assert_eq!(h.state().selected, Some(id), "left click in column {col:?}");
    }
}

#[test]
fn a_right_click_in_any_column_selects_the_row_and_opens_the_menu() {
    let _g = lock();
    let fx = fixture();
    let mut h = harness(&fx.root);
    let cols = h.state().tree_columns();
    for (i, &col) in cols.iter().enumerate() {
        let name = format!("d{}", i % 8 + 1);
        let id = node_named(&h, &name);
        h.state_mut().selected = None;
        h.run_steps(2);
        let name_pos = center_of(&h, &name);
        let x = if col == SortCol::Name { name_pos.x } else { header_x(&h, &h.state().column_caption(col), name_pos.y) };
        click_at(&mut h, Pos2::new(x, name_pos.y), PointerButton::Secondary);
        assert_eq!(h.state().selected, Some(id), "right click in column {col:?}");
        assert!(h.query_by_label("Copy path").is_some(), "context menu in column {col:?}");
        h.key_press(Key::Escape);
        h.run_steps(3);
        assert!(h.query_by_label("Copy path").is_none(), "Escape closes the menu (column {col:?})");
    }
}

#[test]
fn enter_confirms_rename_and_escape_cancels() {
    let _g = lock();
    let fx = fixture();
    let mut h = harness(&fx.root);
    let id = node_named(&h, "d2");

    row_menu(&mut h, "d2", "Rename");
    assert!(h.state().name_dialog.is_some(), "Rename opens the dialog");
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(h.state().name_dialog.is_none(), "Escape cancels");
    assert!(fx.root.join("d2").is_dir());

    row_menu(&mut h, "d2", "Rename");
    h.event(Event::Text("Z".into()));
    h.run_steps(2);
    assert!(h.state().name_dialog.as_ref().unwrap().text.contains('Z'), "typing reaches the field");
    h.state_mut().name_dialog.as_mut().unwrap().text = "renamed".into();
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(4);
    assert!(h.state().name_dialog.is_none(), "Enter closes the dialog");
    assert!(fx.root.join("renamed").is_dir() && !fx.root.join("d2").exists(), "folder renamed on disk");
    assert_eq!(h.state().loaded.as_ref().unwrap().tree.name(id), "renamed");
}

#[test]
fn enter_creates_a_new_folder_and_rejects_bad_names() {
    let _g = lock();
    let fx = fixture();
    let mut h = harness(&fx.root);

    row_menu(&mut h, "d3", "New folder");
    assert!(h.state().name_dialog.as_ref().is_some_and(|d| d.new_folder));
    h.state_mut().name_dialog.as_mut().unwrap().text = "bad/name".into();
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    let d = h.state().name_dialog.as_ref().expect("the dialog stays open on a bad name");
    assert!(!d.err.is_empty(), "an error is shown");
    assert!(!fx.root.join("d3").join("bad").exists());

    h.state_mut().name_dialog.as_mut().unwrap().text = "made".into();
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(4);
    assert!(h.state().name_dialog.is_none());
    assert!(fx.root.join("d3").join("made").is_dir(), "folder created on disk");
}

#[test]
fn arrow_keys_move_the_selection_through_the_rows() {
    let _g = lock();
    let fx = fixture();
    let mut h = harness(&fx.root);
    let p = center_of(&h, "d4");
    click_at(&mut h, p, PointerButton::Primary);
    let ids = |h: &Harness<'_, App>| -> Vec<Option<NodeId>> { h.state().rows.iter().map(|r| if let RowKind::Node(i) = r.kind { Some(i) } else { None }).collect() };
    let start = ids(&h).iter().position(|&i| i == h.state().selected).expect("the clicked row is selected");
    h.key_press(Key::ArrowDown);
    h.run_steps(3);
    assert_eq!(h.state().selected, ids(&h)[start + 1], "Down selects the next row");
    h.key_press(Key::ArrowUp);
    h.run_steps(3);
    assert_eq!(h.state().selected, ids(&h)[start], "Up goes back");
    h.key_press(Key::End);
    h.run_steps(3);
    assert_eq!(h.state().selected, *ids(&h).last().unwrap(), "End selects the last row");
    h.key_press(Key::Home);
    h.run_steps(3);
    assert_eq!(h.state().selected, ids(&h)[0], "Home selects the first row");
}

#[test]
fn a_click_in_any_column_of_the_largest_list_selects_the_file() {
    let _g = lock();
    let fx = fixture();
    let mut h = harness(&fx.root);
    h.state_mut().view = ViewKind::Largest;
    h.run_steps(4);
    let file = h.state().loaded.as_ref().unwrap().tree.resolve_path(&["d8", "f.bin"]).unwrap();
    let path = h.state().loaded.as_ref().unwrap().tree.path_of(file).to_string_lossy().into_owned();
    for (i, caption) in ["#", "Path", "Size", "Last Modified"].into_iter().enumerate() {
        h.state_mut().selected = None;
        h.run_steps(2);
        let row = center_of(&h, &path);
        let x = if i == 1 { row.x } else { header_x(&h, caption, row.y) };
        click_at(&mut h, Pos2::new(x, row.y), PointerButton::Primary);
        assert_eq!(h.state().selected, Some(file), "click in list column '{caption}'");
    }
}

#[test]
fn a_double_click_on_a_file_type_shows_only_those_files_in_the_tree() {
    let _g = lock();
    let fx = fixture();
    let mut h = harness(&fx.root);
    h.state_mut().view = ViewKind::Types;
    h.run_steps(4);
    let row = center_of(&h, ".bin");
    h.event(Event::PointerMoved(row));
    h.run_steps(2);
    // Two clicks one frame apart (well inside the double-click delay).
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(Event::PointerButton { pos: row, button: PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        }
        h.run_steps(1);
    }
    h.run_steps(4);
    assert_eq!(h.state().view, ViewKind::Tree, "back to the tree");
    assert_eq!(h.state().filter.exts, vec!["bin".to_string()]);
    assert!(h.state().show_filters, "the filter bar is shown");
    assert!(h.state().filter_set.is_some(), "the tree is filtered");
}

/// Texts that were drawn partly outside the rectangle they are clipped to (cut off on screen).
fn clipped_texts(output: &egui::FullOutput) -> Vec<String> {
    fn walk(shape: &egui::epaint::Shape, clip: egui::Rect, out: &mut Vec<String>) {
        match shape {
            egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| walk(s, clip, out)),
            egui::epaint::Shape::Text(t) => {
                let bounds = t.galley.rect.translate(t.pos.to_vec2());
                let tolerance = 1.0;
                if !t.galley.text().trim().is_empty() && !clip.expand(tolerance).contains_rect(bounds) {
                    out.push(format!("{:?} (text {:?}, clip {:?})", t.galley.text(), bounds, clip));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, clipped.clip_rect, &mut out);
    }
    out
}

/// Run a few frames of a fresh app in `language` at `size` and return the last frame's output.
fn frames(language: &str, size: egui::Vec2, setup: impl FnOnce(&mut App)) -> egui::FullOutput {
    let ctx = egui::Context::default();
    let st = Settings { language: language.into(), show_map: false, ..Settings::default() };
    let mut app = App::new(&ctx, st, &RunOptions::default());
    app.persist_enabled = false;
    app.settings_path = None;
    setup(&mut app);
    let mut last = None;
    for i in 0..4 {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, size)), time: Some(i as f64 * 0.1), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| app.draw(ui));
        out.textures_delta.clear(); // nothing renders these frames: drop the texture uploads on purpose
        last = Some(out);
    }
    last.unwrap()
}

#[test]
fn no_ribbon_or_dialog_text_is_cut_off_in_english_or_french() {
    let _g = lock();
    ribbon::MISSING_HINTS.with(|m| m.borrow_mut().clear());
    let mut problems = Vec::new();
    for lang in ["en", "fr"] {
        // The default window width (1200): every ribbon tab must fit without scrolling.
        for tab in 0..=4usize {
            let out = frames(lang, egui::vec2(1200.0, 900.0), |a| a.ribbon_tab = tab);
            problems.extend(clipped_texts(&out).into_iter().map(|t| format!("[{lang}] ribbon tab {tab}: {t}")));
        }
        let out = frames(lang, egui::vec2(1200.0, 900.0), |a| a.not_admin = true);
        problems.extend(clipped_texts(&out).into_iter().map(|t| format!("[{lang}] start page admin notice: {t}")));
        let out = frames(lang, egui::vec2(1200.0, 900.0), |a| a.show_options = true);
        problems.extend(clipped_texts(&out).into_iter().map(|t| format!("[{lang}] options window: {t}")));
        let out = frames(lang, egui::vec2(1200.0, 900.0), |a| a.show_about = true);
        problems.extend(clipped_texts(&out).into_iter().map(|t| format!("[{lang}] about window: {t}")));
        for permanently in [false, true] {
            let out = frames(lang, egui::vec2(1200.0, 900.0), |a| {
                a.ask_delete_files(vec![(PathBuf::from("C:/some/folder"), 1234)]);
                a.delete_permanently = permanently;
            });
            problems.extend(clipped_texts(&out).into_iter().map(|t| format!("[{lang}] delete confirmation (permanent {permanently}): {t}")));
        }
    }
    assert!(problems.is_empty(), "cut-off texts:
{}", problems.join("
"));
    let missing = ribbon::MISSING_HINTS.with(|m| m.borrow().clone());
    assert!(missing.is_empty(), "ribbon buttons without an explanation: {missing:?}");
}

#[test]
fn an_excluded_folder_can_be_seen_and_brought_back() {
    let _g = lock();
    let fx = fixture();
    let mut h = harness(&fx.root);
    let p = center_of(&h, "d2");
    click_at(&mut h, p, PointerButton::Primary);
    h.key_press_modifiers(egui::Modifiers { ctrl: true, shift: true, command: true, ..Default::default() }, Key::X);
    h.run_steps(4);
    assert_eq!(h.state().excludes, vec![fx.root.join("d2")], "the folder is excluded");
    assert!(h.query_by_label("d2").is_none(), "and gone from the tree");
    assert!(h.state().notice.contains("d2"), "the status says what happened: {:?}", h.state().notice);

    // The status bar keeps a visible entry that opens the list...
    let link = center_of(&h, "1 Excluded");
    click_at(&mut h, link, PointerButton::Primary);
    assert!(h.state().show_options, "the excluded counter opens the options");
    // ...where the folder can be restored, with a way to scan again.
    let restore = center_of(&h, "Restore");
    click_at(&mut h, restore, PointerButton::Primary);
    assert!(h.state().excludes.is_empty());
    assert!(h.query_by_label("Rescan now").is_some(), "a rescan is offered");
}

#[test]
fn parent_folder_selects_the_parent_in_the_tree() {
    let _g = lock();
    let fx = fixture();
    let mut h = harness(&fx.root);
    let p = center_of(&h, "d3");
    click_at(&mut h, p, PointerButton::Primary);
    let d3 = node_named(&h, "d3");
    assert_eq!(h.state().selected, Some(d3));
    assert!(h.state().can_go_up());
    let ctx = h.ctx.clone();
    h.state_mut().run_cmd(&ctx, Cmd::ParentFolder);
    h.run_steps(2);
    assert_eq!(h.state().selected, Some(Tree::ROOT), "the parent of d3 is the scanned folder");
    assert!(h.state().handle.is_none(), "no new scan was started");
}

#[test]
fn labels_are_not_selectable_so_the_cursor_stays_an_arrow() {
    let _g = lock();
    let ctx = egui::Context::default();
    let _app = App::new(&ctx, Settings { language: "en".into(), ..Settings::default() }, &RunOptions::default());
    assert!(!ctx.style_of(egui::Theme::Dark).interaction.selectable_labels, "selectable labels show the text-selection cursor");
}

#[test]
fn ribbon_captions_are_only_ever_broken_between_words() {
    let _g = lock();
    let ctx = egui::Context::default();
    for lang in ["en", "fr"] {
        let _app = App::new(&ctx, Settings { language: lang.into(), ..Settings::default() }, &RunOptions::default());
        let font = egui::FontId::proportional(11.0);
        let mut bad = Vec::new();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            for (caption, _) in ribbon::HINTS {
                let label = tr(caption);
                let lines = ribbon::split_caption(ui, &label, &font, 60.0);
                if lines.len() > 2 || lines.join(" ") != label {
                    bad.push(format!("{label:?} -> {lines:?}"));
                }
            }
        });
        out.textures_delta.clear();
        assert!(bad.is_empty(), "[{lang}] captions broken inside a word or in too many lines: {bad:#?}");
    }
}

/// Click the labelled button and let a few frames pass.
fn click_label(h: &mut Harness<'_, App>, label: &str) {
    let p = center_of(h, label);
    click_at(h, p, PointerButton::Primary);
}

fn wait_for_delete(h: &mut Harness<'_, App>) {
    for _ in 0..300 {
        h.step();
        if h.state().delete_job.is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(h.state().delete_job.is_none(), "the deletion did not finish");
    h.run_steps(3);
}

#[test]
fn the_cleanup_list_ticks_the_safe_rows_and_deletes_them_permanently() {
    let _g = lock();
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    let put = |p: &str, n: usize| {
        let f = r.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, vec![1u8; n]).unwrap();
    };
    put("web/package.json", 10);
    put("web/package-lock.json", 10);
    put("web/node_modules/dep/index.js", 9000); // safe: lock file next to it
    put("crate/Cargo.toml", 10);
    put("crate/target/debug/a.bin", 8000); // safe
    put("loose/package.json", 10);
    put("loose/node_modules/x/i.js", 7000); // check first: no lock file
    let mut h = harness(r);
    h.state_mut().view = ViewKind::Cleanup;
    h.run_steps(4);
    assert_eq!((h.get_all_by_label("Safe").count(), h.get_all_by_label("Check first").count()), (2, 1), "both safety levels are listed");

    click_label(&mut h, "Tick the safe ones");
    assert_eq!(h.state().cleanup_checked.len(), 2, "only the two safe folders are ticked");
    let safe_paths: Vec<_> = ["web/node_modules", "crate/target"].iter().map(|p| r.join(p)).collect();

    let button = h.get_by_label_contains("Delete ticked").rect().center();
    click_at(&mut h, button, PointerButton::Primary);
    assert!(h.state().confirm.is_some(), "a confirmation is asked first");
    assert!(safe_paths.iter().all(|p| p.exists()), "nothing is deleted before the confirmation");

    click_label(&mut h, "Delete permanently (skip the Recycle Bin)");
    assert!(h.state().delete_permanently);
    click_label(&mut h, "Delete");
    wait_for_delete(&mut h);

    assert!(safe_paths.iter().all(|p| !p.exists()), "the safe folders are gone from the disk");
    assert!(r.join("loose/node_modules/x/i.js").exists(), "the check-first folder is untouched");
    assert!(r.join("web/package.json").exists() && r.join("crate/Cargo.toml").exists(), "project files stay");
    assert!(h.state().cleanup_checked.is_empty());
    assert!(h.state().notice.starts_with("Deleted: 2 - 16.6"), "two folders, 17,000 bytes: {:?}", h.state().notice);
    assert!(h.state().delete_error.is_none());
}

#[test]
fn permanent_deletion_refuses_the_profile_folder_and_leaves_it_alone() {
    let _g = lock();
    let fx = fixture();
    let mut h = harness(&fx.root);
    let home = std::path::PathBuf::from(std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).expect("a home folder"));
    h.state_mut().ask_delete_files(vec![(home.clone(), 0)]);
    h.run_steps(3);
    assert!(h.state().confirm.is_some());
    click_label(&mut h, "Delete permanently (skip the Recycle Bin)");
    click_label(&mut h, "Delete");
    wait_for_delete(&mut h);
    assert!(home.exists(), "the folder is still there");
    assert!(h.state().delete_error.as_ref().is_some_and(|e| e.contains("protected")), "{:?}", h.state().delete_error);
}

/// A start page (no scan) of an app that believes it runs without administrator rights.
fn start_harness(not_admin: bool) -> Harness<'static, App> {
    let ctx = egui::Context::default();
    let st = Settings { language: "en".into(), ..Settings::default() };
    let mut app = App::new(&ctx, st, &RunOptions::default());
    app.persist_enabled = false;
    app.settings_path = None;
    app.not_admin = not_admin;
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(0.05).build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    h.run_steps(4);
    h
}

#[test]
fn the_start_page_warns_once_when_not_running_as_administrator() {
    let _g = lock();
    let h = start_harness(false);
    assert!(h.query_by_label_contains("without administrator rights").is_none(), "an administrator run has nothing to warn about");
    let mut h = start_harness(true);
    assert!(h.query_by_label_contains("without administrator rights").is_some());
    assert!(h.query_by_label_contains("fast NTFS scan").is_some(), "says what administrator mode brings");
    let p = center_of(&h, "Don't show again");
    click_at(&mut h, p, PointerButton::Primary);
    assert!(!h.state().settings.warn_not_admin, "the choice is kept in the settings");
    assert!(h.query_by_label_contains("without administrator rights").is_none(), "and the notice goes away");
}

#[test]
fn the_after_scan_banner_says_what_is_missing_and_can_open_the_list() {
    let _g = lock();
    let fx = fixture();
    let mut h = harness(&fx.root);
    h.state_mut().admin_hint = Some(AdminHint { denied: 1234, missing: Some((800 << 30, 700 << 30)) });
    h.run_steps(3);
    assert!(h.query_by_label_contains("may be incomplete").is_some());
    assert!(h.query_by_label_contains("1,234").is_some(), "the number of unreadable folders");
    assert!(h.query_by_label_contains("The drive reports").is_some(), "the gap between the drive and the scan");
    assert!(h.query_by_label_contains("800").is_some() && h.query_by_label_contains("700").is_some(), "both figures are shown");
    let right = h.get_by_label_contains("may be incomplete").rect().right();
    assert!(right <= 1200.0, "the banner fits the window ({right})");
    let p = center_of(&h, "Show the folders");
    click_at(&mut h, p, PointerButton::Primary);
    assert!(h.state().show_errors, "the list of folders opens");
    let p = center_of(&h, "Dismiss");
    click_at(&mut h, p, PointerButton::Primary);
    assert!(h.state().admin_hint.is_none());
    assert!(h.query_by_label_contains("may be incomplete").is_none());
}

#[test]
fn the_administrator_notice_texts_are_translated() {
    for t in [
        "This result may be incomplete: Strata is not running as administrator.",
        "The drive reports {used} in use, but only {found} was found.",
        "Show the folders",
        "Strata is running without administrator rights.",
        "Some folders (other users' profiles, System Volume Information, parts of Windows) cannot be read, so sizes can be lower than what the drive really holds. Administrator mode also allows the fast NTFS scan.",
        "Don't show again",
        "You can turn this reminder back on in Options.",
        "Warn when not running as administrator",
    ] {
        assert!(crate::i18n::is_translated("fr", t), "no French for: {t}");
    }
}
