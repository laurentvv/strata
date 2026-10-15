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
