//! SVG rail glyphs, rasterized once at startup.
//!
//! The raster is deliberately small: the picker downscales it to the
//! button's cell area, and bright/muted protocol pairs let hover and
//! active states reuse the same artwork.

use image::{DynamicImage, RgbaImage};
use ratatui::style::Color;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;

use super::NavRailItem;
use crate::theme::Theme;

/// Pixel edge of the source raster. Comfortably above the 4x2 cell
/// target so antialiasing survives the downscale.
const ICON_PX: u32 = 64;
/// Muted variant factor, matching the theme's muted text on white.
const MUTED: f32 = 0.5;

pub(crate) struct RailIcons {
    pub(crate) home: IconVariants,
    pub(crate) library: IconVariants,
    pub(crate) command_palette: IconVariants,
    pub(crate) settings: IconVariants,
}

/// One protocol per button surface: idle, hover/active, and the
/// brighter active-hover fill. Halfblocks drops alpha, so the rasters
/// are composited onto the exact button background up front.
pub(crate) struct IconVariants {
    pub(crate) idle: StatefulProtocol,
    pub(crate) surface: StatefulProtocol,
    pub(crate) active_hover: StatefulProtocol,
}

impl RailIcons {
    pub(crate) fn load(picker: &Picker, theme: &Theme) -> Result<Self, String> {
        let backgrounds = [
            rgb(theme.background),
            rgb(theme.surface),
            rgb(theme.keycap.on_surface),
        ];
        Ok(Self {
            home: IconVariants::load(picker, HOME_SVG, backgrounds)?,
            library: IconVariants::load(picker, LIBRARY_SVG, backgrounds)?,
            command_palette: IconVariants::load(picker, COMMAND_PALETTE_SVG, backgrounds)?,
            settings: IconVariants::load(picker, SETTINGS_SVG, backgrounds)?,
        })
    }

    pub(crate) fn for_item_mut(&mut self, item: NavRailItem) -> &mut IconVariants {
        match item {
            NavRailItem::Home => &mut self.home,
            NavRailItem::Library => &mut self.library,
            NavRailItem::CommandPalette => &mut self.command_palette,
            NavRailItem::Settings => &mut self.settings,
        }
    }
}

impl IconVariants {
    fn load(picker: &Picker, svg: &str, backgrounds: [[u8; 3]; 3]) -> Result<Self, String> {
        let bright = rasterize(svg)?;
        let muted = tint(&bright, MUTED);
        Ok(Self {
            idle: picker.new_resize_protocol(on_background(&muted, backgrounds[0])),
            surface: picker.new_resize_protocol(on_background(&bright, backgrounds[1])),
            active_hover: picker.new_resize_protocol(on_background(&bright, backgrounds[2])),
        })
    }
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

const SETTINGS_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" fill="#ffffff" viewBox="0 0 256 256"><path d="M237.94,107.21a8,8,0,0,0-3.89-5.4l-29.83-17-.12-33.62a8,8,0,0,0-2.83-6.08,111.91,111.91,0,0,0-36.72-20.67,8,8,0,0,0-6.46.59L128,41.85,97.88,25a8,8,0,0,0-6.47-.6A111.92,111.92,0,0,0,54.73,45.15a8,8,0,0,0-2.83,6.07l-.15,33.65-29.83,17a8,8,0,0,0-3.89,5.4,106.47,106.47,0,0,0,0,41.56,8,8,0,0,0,3.89,5.4l29.83,17,.12,33.63a8,8,0,0,0,2.83,6.08,111.91,111.91,0,0,0,36.72,20.67,8,8,0,0,0,6.46-.59L128,214.15,158.12,231a7.91,7.91,0,0,0,3.9,1,8.09,8.09,0,0,0,2.57-.42,112.1,112.1,0,0,0,36.68-20.73,8,8,0,0,0,2.83-6.07l.15-33.65,29.83-17a8,8,0,0,0,3.89-5.4A106.47,106.47,0,0,0,237.94,107.21ZM128,168a40,40,0,1,1,40-40A40,40,0,1,1,128,168Z"/></svg>"##;

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
    fn icons_build_protocols_for_every_item() {
        let picker = Picker::halfblocks();
        let mut icons =
            RailIcons::load(&picker, &crate::theme::default::DARK_THEME).expect("icons load");
        for item in [
            NavRailItem::Home,
            NavRailItem::Library,
            NavRailItem::CommandPalette,
            NavRailItem::Settings,
        ] {
            let _ = icons.for_item_mut(item);
        }
    }
}
