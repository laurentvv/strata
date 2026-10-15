//! Embedded program icon.

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

static MAIN_ICON: &[u8] = include_bytes!("../../assets/icon.ico");

/// Decode the window icon (largest image of the .ico) to RGBA.
pub fn window_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory_with_format(MAIN_ICON, image::ImageFormat::Ico).ok()?.to_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData { rgba: img.into_raw(), width, height })
}

/// Texture of the program icon (About box).
pub fn icon_texture(ctx: &egui::Context) -> Option<TextureHandle> {
    let icon = window_icon()?;
    let img = ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba);
    Some(ctx.load_texture("app_icon", img, TextureOptions::LINEAR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icon_decodes() {
        let icon = window_icon().expect("ico decodes");
        assert!(icon.width >= 128 && icon.rgba.len() == (icon.width * icon.height * 4) as usize);
        assert!(icon.rgba.chunks(4).any(|p| p[3] > 0), "icon has visible pixels");
    }
}
