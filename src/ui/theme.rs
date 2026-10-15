//! The Strata look: deep slate surfaces, teal for data, amber for emphasis. One place for every
//! colour so the dark and light variants stay consistent.

use eframe::egui::{Color32, Stroke, Visuals};

pub const TEAL: Color32 = Color32::from_rgb(43, 181, 160);
pub const AMBER: Color32 = Color32::from_rgb(242, 163, 58);

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Egui visuals for the dark or the light variant.
pub fn visuals(dark: bool) -> Visuals {
    if dark {
        let mut v = Visuals::dark();
        v.panel_fill = rgb(18, 23, 30);
        v.window_fill = rgb(24, 30, 39);
        v.extreme_bg_color = rgb(13, 17, 23);
        v.faint_bg_color = rgb(22, 28, 36);
        v.window_stroke = Stroke::new(1.0, rgb(52, 64, 80));
        v.selection.bg_fill = rgb(30, 84, 88);
        v.selection.stroke = Stroke::new(1.0, rgb(150, 235, 216));
        v.hyperlink_color = AMBER;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, rgb(42, 52, 66));
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, rgb(196, 206, 218));
        v.widgets.inactive.weak_bg_fill = rgb(29, 37, 48);
        v.widgets.inactive.bg_fill = rgb(29, 37, 48);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, rgb(206, 215, 226));
        v.widgets.hovered.weak_bg_fill = rgb(38, 50, 64);
        v.widgets.hovered.bg_fill = rgb(38, 50, 64);
        v.widgets.hovered.fg_stroke = Stroke::new(1.5, Color32::WHITE);
        v.widgets.active.weak_bg_fill = rgb(30, 84, 88);
        v.widgets.active.bg_fill = rgb(30, 84, 88);
        v
    } else {
        let mut v = Visuals::light();
        v.panel_fill = rgb(243, 246, 248);
        v.window_fill = rgb(252, 253, 254);
        v.extreme_bg_color = Color32::WHITE;
        v.faint_bg_color = rgb(234, 239, 243);
        v.selection.bg_fill = rgb(176, 226, 216);
        v.selection.stroke = Stroke::new(1.0, rgb(14, 100, 90));
        v.hyperlink_color = rgb(170, 94, 8);
        v.widgets.hovered.weak_bg_fill = rgb(214, 232, 230);
        v.widgets.inactive.weak_bg_fill = rgb(226, 232, 238);
        v
    }
}

/// Left / right colours of the "% of parent" bar.
pub fn percent_bar(dark: bool) -> (Color32, Color32) {
    if dark {
        (rgb(22, 104, 96), TEAL)
    } else {
        (rgb(112, 200, 188), rgb(52, 168, 148))
    }
}

/// Size bar drawn behind the folder name; the three biggest siblings are brighter.
pub fn name_bar(dark: bool, bold: bool) -> Color32 {
    match (dark, bold) {
        (true, false) => rgb(34, 54, 62),
        (true, true) => rgb(40, 92, 96),
        (false, false) => rgb(208, 228, 228),
        (false, true) => rgb(146, 206, 198),
    }
}

/// Fill of the proportional map by nesting level (level 0 darkest).
pub const MAP_LEVELS: [Color32; 5] = [rgb(14, 34, 46), rgb(20, 62, 72), rgb(28, 96, 100), rgb(44, 134, 128), rgb(86, 176, 150)];

/// Tile for files (aggregated block or single file): the level colour warmed towards amber so
/// files and folders read differently at a glance.
pub fn file_tile(level: Color32) -> Color32 {
    let mix = |a: u8, b: u8| (a as f32 * 0.28 + b as f32 * 0.72) as u8;
    Color32::from_rgb(mix(level.r(), 214), mix(level.g(), 152), mix(level.b(), 64))
}

pub const MAP_FREE: Color32 = rgb(52, 60, 72);
pub const MAP_BORDER: Color32 = rgb(8, 16, 22);

/// Clean-up safety labels: green for safe to delete, amber for "check first".
pub const RISK_SAFE: Color32 = rgb(96, 190, 120);
pub const RISK_CHECK: Color32 = rgb(236, 180, 70);
pub const MAP_SELECTED: Color32 = rgb(255, 214, 90);

/// Colour of bars in charts (age buckets, size history, drive usage).
pub const SERIES: Color32 = TEAL;
pub const SERIES_DIM: Color32 = rgb(30, 128, 116);
pub const SERIES_LATEST: Color32 = AMBER;
pub const DANGER: Color32 = rgb(210, 70, 60);
pub const FILE_TAB: Color32 = rgb(24, 118, 108);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_tiles_are_warmer_than_their_level() {
        for lvl in MAP_LEVELS {
            let f = file_tile(lvl);
            assert!(f.r() > lvl.r() && f.b() < 120, "{lvl:?} -> {f:?}");
        }
    }

    #[test]
    fn map_levels_get_lighter() {
        let lum = |c: Color32| c.r() as u32 + c.g() as u32 + c.b() as u32;
        assert!(MAP_LEVELS.windows(2).all(|w| lum(w[0]) < lum(w[1])));
    }

    #[test]
    fn variants_differ() {
        assert_ne!(visuals(true).panel_fill, visuals(false).panel_fill);
        assert!(visuals(true).dark_mode && !visuals(false).dark_mode);
    }
}
