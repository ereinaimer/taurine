//! SVG rail glyphs, rasterized once at startup.
//!
//! The raster is intentionally oversized so the picker's high-quality
//! downscale keeps the edges smooth at the small rail size. Bright and
//! muted protocol pairs let hover/active states reuse the artwork
//! without painting a background behind it.

use image::{DynamicImage, RgbaImage};
use ratatui::layout::Size;
use ratatui::style::Color;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::{FilterType, FontSize, Resize};

use super::NavRailItem;
use crate::theme::Theme;

/// Pixel edge of the source raster. Well above the few cells used on
/// screen so downscaling has plenty of detail to sample.
const ICON_PX: u32 = 256;
/// Muted variant factor, matching the theme's muted text on white.
const MUTED: f32 = 0.5;

pub(crate) struct RailIcons {
    pub(crate) home: IconVariants,
    pub(crate) library: IconVariants,
    pub(crate) command_palette: IconVariants,
    pub(crate) settings: IconVariants,
}

/// Muted and bright protocols; the button itself never paints a fill.
/// Halfblocks drop alpha, so only that protocol gets the rail color
/// merged into transparent pixels.
pub(crate) struct IconVariants {
    pub(crate) idle: Protocol,
    pub(crate) bright: Protocol,
}

impl RailIcons {
    pub(crate) fn load(picker: &Picker, theme: &Theme) -> Result<Self, String> {
        let halfblocks = picker.protocol_type() == ProtocolType::Halfblocks;
        let rail_bg = rgb(theme.background);
        Ok(Self {
            home: IconVariants::load(picker, HOME_SVG, halfblocks, rail_bg)?,
            library: IconVariants::load(picker, LIBRARY_SVG, halfblocks, rail_bg)?,
            command_palette: IconVariants::load(picker, COMMAND_PALETTE_SVG, halfblocks, rail_bg)?,
            settings: IconVariants::load(picker, SETTINGS_SVG, halfblocks, rail_bg)?,
        })
    }

    pub(crate) fn for_item(&self, item: NavRailItem) -> &IconVariants {
        match item {
            NavRailItem::Home => &self.home,
            NavRailItem::Library => &self.library,
            NavRailItem::CommandPalette => &self.command_palette,
            NavRailItem::Settings => &self.settings,
        }
    }
}

impl IconVariants {
    fn load(picker: &Picker, svg: &str, opaque: bool, background: [u8; 3]) -> Result<Self, String> {
        let bright = rasterize(svg)?;
        let muted = tint(&bright, MUTED);
        let (idle, bright) = if opaque {
            (
                on_background(&muted, background),
                on_background(&bright, background),
            )
        } else {
            let font = picker.font_size();
            (
                half_size_canvas(&muted, font),
                half_size_canvas(&bright, font),
            )
        };
        Ok(Self {
            idle: protocol(picker, idle)?,
            bright: protocol(picker, bright)?,
        })
    }
}

fn protocol(picker: &Picker, image: DynamicImage) -> Result<Protocol, String> {
    picker
        .new_protocol(
            image,
            Size::new(4, 2),
            Resize::Fit(Some(FilterType::Lanczos3)),
        )
        .map_err(|error| error.to_string())
}

fn rasterize(svg: &str) -> Result<DynamicImage, String> {
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(svg, &options).map_err(|error| error.to_string())?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return Err("rail icon SVG has no size".to_string());
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(ICON_PX, ICON_PX)
        .ok_or_else(|| "failed to allocate the rail icon raster".to_string())?;
    let transform = resvg::tiny_skia::Transform::from_scale(
        ICON_PX as f32 / size.width(),
        ICON_PX as f32 / size.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let image = RgbaImage::from_raw(ICON_PX, ICON_PX, pixmap.take())
        .ok_or_else(|| "failed to wrap the rail icon raster".to_string())?;
    Ok(DynamicImage::ImageRgba8(image))
}

fn tint(image: &DynamicImage, factor: f32) -> DynamicImage {
    let mut rgba = image.to_rgba8();
    for pixel in rgba.pixels_mut() {
        for channel in &mut pixel.0[..3] {
            *channel = (f32::from(*channel) * factor) as u8;
        }
    }
    DynamicImage::ImageRgba8(rgba)
}

/// Compose the icon at exactly half the button's pixel area, centered
/// on the full 4x2 canvas. Working in the picker's pixel dimensions
/// keeps the artwork's aspect and avoids the fit padding landing in a
/// corner.
fn half_size_canvas(image: &DynamicImage, font: FontSize) -> DynamicImage {
    let canvas_w = u32::from(font.width) * 4;
    let canvas_h = u32::from(font.height) * 2;
    let glyph_w = u32::from(font.width) * 2;
    let glyph_h = u32::from(font.height);
    let scale = (glyph_w as f32 / image.width() as f32).min(glyph_h as f32 / image.height() as f32);
    let target_w = (image.width() as f32 * scale).round().max(1.0) as u32;
    let target_h = (image.height() as f32 * scale).round().max(1.0) as u32;
    let glyph = image.resize_exact(target_w, target_h, FilterType::Lanczos3);
    let mut canvas = RgbaImage::new(canvas_w, canvas_h);
    let x = canvas_w.saturating_sub(target_w) / 2;
    let y = canvas_h.saturating_sub(target_h) / 2;
    image::imageops::overlay(&mut canvas, &glyph, i64::from(x), i64::from(y));
    DynamicImage::ImageRgba8(canvas)
}

fn on_background(image: &DynamicImage, background: [u8; 3]) -> DynamicImage {
    let mut rgba = image.to_rgba8();
    for pixel in rgba.pixels_mut() {
        let alpha = f32::from(pixel[3]) / 255.0;
        for (channel, background) in pixel.0[..3].iter_mut().zip(background) {
            *channel =
                (f32::from(*channel) * alpha + f32::from(background) * (1.0 - alpha)).round() as u8;
        }
        pixel[3] = 255;
    }
    DynamicImage::ImageRgba8(rgba)
}

fn rgb(color: Color) -> [u8; 3] {
    match color {
        Color::Rgb(red, green, blue) => [red, green, blue],
        _ => [0, 0, 0],
    }
}

const HOME_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><path fill="#ffffff" d="M341.8 72.6C329.5 61.2 310.5 61.2 298.3 72.6L74.3 280.6C64.7 289.6 61.5 303.5 66.3 315.7C71.1 327.9 82.8 336 96 336L112 336L112 512C112 547.3 140.7 576 176 576L464 576C499.3 576 528 547.3 528 512L528 336L544 336C557.2 336 569 327.9 573.8 315.7C578.6 303.5 575.4 289.5 565.8 280.6L341.8 72.6zM304 384L336 384C362.5 384 384 405.5 384 432L384 528L256 528L256 432C256 405.5 277.5 384 304 384z"/></svg>"##;

const LIBRARY_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" fill="#ffffff" viewBox="0 0 256 256"><path d="M184,32H72A16,16,0,0,0,56,48V224a8,8,0,0,0,12.24,6.78L128,193.43l59.77,37.35A8,8,0,0,0,200,224V48A16,16,0,0,0,184,32Z"/></svg>"##;

const SETTINGS_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><path fill="#ffffff" d="M495.9 166.6c3.2 8.7.5 18.4-6.4 24.6l-43.3 39.4c1.1 8.3 1.7 16.8 1.7 25.4s-.6 17.1-1.7 25.4l43.3 39.4c6.9 6.2 9.6 15.9 6.4 24.6-4.4 11.9-9.7 23.3-15.8 34.3l-4.7 8.1c-6.6 11-14 21.4-22.1 31.2-5.9 7.2-15.7 9.6-24.5 6.8l-55.7-17.7c-13.4 10.3-28.2 18.9-44 25.4l-12.5 57.1c-2 9.1-9 16.3-18.2 17.8-13.8 2.3-28 3.5-42.5 3.5s-28.7-1.2-42.5-3.5c-9.2-1.5-16.2-8.7-18.2-17.8l-12.5-57.1c-15.8-6.5-30.6-15.1-44-25.4L83.1 425.9c-8.8 2.8-18.6.3-24.5-6.8-8.1-9.8-15.5-20.2-22.1-31.2l-4.7-8.1c-6.1-11-11.4-22.4-15.8-34.3-3.2-8.7-.5-18.4 6.4-24.6l43.3-39.4C64.6 273.1 64 264.6 64 256s.6-17.1 1.7-25.4L22.4 191.2c-6.9-6.2-9.6-15.9-6.4-24.6 4.4-11.9 9.7-23.3 15.8-34.3l4.7-8.1c6.6-11 14-21.4 22.1-31.2 5.9-7.2 15.7-9.6 24.5-6.8l55.7 17.7c13.4-10.3 28.2-18.9 44-25.4l12.5-57.1c2-9.1 9-16.3 18.2-17.8C227.3 1.2 241.5 0 256 0s28.7 1.2 42.5 3.5c9.2 1.5 16.2 8.7 18.2 17.8l12.5 57.1c15.8 6.5 30.6 15.1 44 25.4l55.7-17.7c8.8-2.8 18.6-.3 24.5 6.8 8.1 9.8 15.5 20.2 22.1 31.2l4.7 8.1c6.1 11 11.4 22.4 15.8 34.3zM256 336a80 80 0 1 0 0-160 80 80 0 1 0 0 160z"/></svg>"##;

const COMMAND_PALETTE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><path fill="#ffffff" d="M73.4 182.6C60.9 170.1 60.9 149.8 73.4 137.3C85.9 124.8 106.2 124.8 118.7 137.3L278.7 297.3C291.2 309.8 291.2 330.1 278.7 342.6L118.7 502.6C106.2 515.1 85.9 515.1 73.4 502.6C60.9 490.1 60.9 469.8 73.4 457.3L210.7 320L73.4 182.6zM288 448L544 448C561.7 448 576 462.3 576 480C576 497.7 561.7 512 544 512L288 512C270.3 512 256 497.7 256 480C256 462.3 270.3 448 288 448z"/></svg>"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_svg_rasterizes_with_visible_pixels() {
        for svg in [HOME_SVG, LIBRARY_SVG, COMMAND_PALETTE_SVG, SETTINGS_SVG] {
            let image = rasterize(svg).expect("svg renders");
            assert_eq!(image.width(), ICON_PX);
            assert_eq!(image.height(), ICON_PX);
            assert!(
                image.to_rgba8().pixels().any(|pixel| pixel[3] > 0),
                "transparent raster"
            );
        }
    }

    #[test]
    fn half_size_canvas_centers_the_icon_at_half_the_cell_area() {
        let font = Picker::halfblocks().font_size();
        let image = rasterize(HOME_SVG).expect("svg renders");
        let padded = half_size_canvas(&image, font);
        assert_eq!(padded.width(), u32::from(font.width) * 4);
        assert_eq!(padded.height(), u32::from(font.height) * 2);
        let rgba = padded.to_rgba8();
        assert_eq!(rgba.get_pixel(0, 0)[3], 0, "corner is transparent");
        assert!(
            rgba.get_pixel(u32::from(font.width) * 2, u32::from(font.height),)[3] > 0,
            "center holds the artwork"
        );
    }

    #[test]
    fn icons_build_protocols_for_every_item() {
        let picker = Picker::halfblocks();
        let icons =
            RailIcons::load(&picker, &crate::theme::default::DARK_THEME).expect("icons load");
        for icon in [
            &icons.home,
            &icons.library,
            &icons.command_palette,
            &icons.settings,
        ] {
            assert_eq!(icon.idle.size(), Size::new(4, 2));
            assert_eq!(icon.bright.size(), Size::new(4, 2));
        }
    }
}
