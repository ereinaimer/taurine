//! SVG rail glyphs, rasterized once at startup.
//!
//! Each SVG is scaled as a vector and rendered by resvg directly into
//! the exact pixel canvas of its 4x2 cell protocol: one vector-to-pixels
//! step with anti-aliased edges, no intermediate bitmap and no resample
//! pass. The raster already matches the cell area, so the documented
//! `Picker::new_protocol(.., Resize::Fit(None))` quick start performs no
//! resize at all and the pixels reach the terminal 1:1. Icons sit
//! slightly inside the button, scaled uniformly, so their aspect ratio
//! never changes.

use image::{DynamicImage, RgbaImage};
use ratatui::layout::Size;
use ratatui::style::Color;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::{FontSize, Resize};

use super::NavRailItem;
use crate::theme::Theme;

/// Muted variant factor, matching the theme's muted text on white.
const MUTED: f32 = 0.5;
/// Graphics icons occupy 5/8 of the 4x2 button canvas; halfblocks fill
/// it so their coarse block grid keeps enough detail.
const ICON_SCALE_NUM: u32 = 5;
const ICON_SCALE_DEN: u32 = 8;

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
        let font = picker.font_size();
        let bright = render_canvas(svg, font, opaque)?;
        let muted = tint(&bright, MUTED);
        let (idle, bright) = if opaque {
            (
                on_background(&muted, background),
                on_background(&bright, background),
            )
        } else {
            (muted, bright)
        };
        Ok(Self {
            idle: protocol(picker, idle)?,
            bright: protocol(picker, bright)?,
        })
    }
}

fn protocol(picker: &Picker, image: DynamicImage) -> Result<Protocol, String> {
    picker
        .new_protocol(image, Size::new(4, 2), Resize::Fit(None))
        .map_err(|error| error.to_string())
}

/// Scale the SVG itself and rasterize it in one resvg step directly at
/// the exact pixel size of the 4x2 cell canvas. Graphics icons sit at
/// 5/8 of the button, centered, and are scaled uniformly so their
/// aspect ratio never changes. Halfblocks fill the canvas because their
/// block grid needs the extra detail. Because the raster matches the
/// cell area exactly, ratatui-image's `Resize::Fit` skips its resize
/// and the protocol receives these pixels unresampled.
fn render_canvas(svg: &str, font: FontSize, opaque: bool) -> Result<DynamicImage, String> {
    let canvas_w = u32::from(font.width).saturating_mul(4);
    let canvas_h = u32::from(font.height).saturating_mul(2);
    if canvas_w == 0 || canvas_h == 0 {
        return Err("terminal reports a zero-size cell".to_string());
    }
    let (glyph_w, glyph_h) = if opaque {
        (canvas_w, canvas_h)
    } else {
        (
            canvas_w.saturating_mul(ICON_SCALE_NUM) / ICON_SCALE_DEN,
            canvas_h.saturating_mul(ICON_SCALE_NUM) / ICON_SCALE_DEN,
        )
    };
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(svg, &options).map_err(|error| error.to_string())?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return Err("rail icon SVG has no size".to_string());
    }
    let scale = (glyph_w as f32 / size.width()).min(glyph_h as f32 / size.height());
    let tx = (canvas_w as f32 - size.width() * scale) / 2.0;
    let ty = (canvas_h as f32 - size.height() * scale) / 2.0;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(canvas_w, canvas_h)
        .ok_or_else(|| "failed to allocate the rail icon raster".to_string())?;
    let transform = resvg::tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, tx, ty);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let image = RgbaImage::from_raw(canvas_w, canvas_h, pixmap.take())
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

const LIBRARY_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" height="24px" viewBox="0 -960 960 960" width="24px" fill="#FFFFFF"><path d="M430-410h89.62q12.75 0 21.37-8.63 8.62-8.63 8.62-21.38 0-12.76-8.62-21.37-8.62-8.62-21.37-8.62H430q-12.75 0-21.38 8.63-8.62 8.63-8.62 21.38 0 12.76 8.62 21.37Q417.25-410 430-410Zm0-120h239.23q12.75 0 21.37-8.63 8.63-8.63 8.63-21.38 0-12.76-8.63-21.37-8.62-8.62-21.37-8.62H430q-12.75 0-21.38 8.63-8.62 8.63-8.62 21.38 0 12.76 8.62 21.37Q417.25-530 430-530Zm0-120h239.23q12.75 0 21.37-8.63 8.63-8.63 8.63-21.38 0-12.76-8.63-21.37-8.62-8.62-21.37-8.62H430q-12.75 0-21.38 8.63-8.62 8.63-8.62 21.38 0 12.76 8.62 21.37Q417.25-650 430-650ZM322.31-260Q292-260 271-281q-21-21-21-51.31v-455.38Q250-818 271-839q21-21 51.31-21h455.38Q808-860 829-839q21 21 21 51.31v455.38Q850-302 829-281q-21 21-51.31 21H322.31Zm-140 140Q152-120 131-141q-21-21-21-51.31v-485.38q0-12.75 8.63-21.38 8.63-8.62 21.38-8.62 12.76 0 21.38 8.62 8.61 8.63 8.61 21.38v485.38q0 4.62 3.85 8.46 3.84 3.85 8.46 3.85h485.38q12.75 0 21.38 8.63 8.62 8.63 8.62 21.38 0 12.76-8.62 21.37-8.63 8.62-21.38 8.62H182.31Z"/></svg>"##;

const SETTINGS_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><path fill="#ffffff" d="M259.1 73.5C262.1 58.7 275.2 48 290.4 48L350.2 48C365.4 48 378.5 58.7 381.5 73.5L396 143.5C410.1 149.5 423.3 157.2 435.3 166.3L503.1 143.8C517.5 139 533.3 145 540.9 158.2L570.8 210C578.4 223.2 575.7 239.8 564.3 249.9L511 297.3C511.9 304.7 512.3 312.3 512.3 320C512.3 327.7 511.8 335.3 511 342.7L564.4 390.2C575.8 400.3 578.4 417 570.9 430.1L541 481.9C533.4 495 517.6 501.1 503.2 496.3L435.4 473.8C423.3 482.9 410.1 490.5 396.1 496.6L381.7 566.5C378.6 581.4 365.5 592 350.4 592L290.6 592C275.4 592 262.3 581.3 259.3 566.5L244.9 496.6C230.8 490.6 217.7 482.9 205.6 473.8L137.5 496.3C123.1 501.1 107.3 495.1 99.7 481.9L69.8 430.1C62.2 416.9 64.9 400.3 76.3 390.2L129.7 342.7C128.8 335.3 128.4 327.7 128.4 320C128.4 312.3 128.9 304.7 129.7 297.3L76.3 249.8C64.9 239.7 62.3 223 69.8 209.9L99.7 158.1C107.3 144.9 123.1 138.9 137.5 143.7L205.3 166.2C217.4 157.1 230.6 149.5 244.6 143.4L259.1 73.5zM320.3 400C364.5 399.8 400.2 363.9 400 319.7C399.8 275.5 363.9 239.8 319.7 240C275.5 240.2 239.8 276.1 240 320.3C240.2 364.5 276.1 400.2 320.3 400z"/></svg>"##;

const COMMAND_PALETTE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><path fill="#ffffff" d="M73.4 182.6C60.9 170.1 60.9 149.8 73.4 137.3C85.9 124.8 106.2 124.8 118.7 137.3L278.7 297.3C291.2 309.8 291.2 330.1 278.7 342.6L118.7 502.6C106.2 515.1 85.9 515.1 73.4 502.6C60.9 490.1 60.9 469.8 73.4 457.3L210.7 320L73.4 182.6zM288 448L544 448C561.7 448 576 462.3 576 480C576 497.7 561.7 512 544 512L288 512C270.3 512 256 497.7 256 480C256 462.3 270.3 448 288 448z"/></svg>"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_svg_rasterizes_with_visible_pixels() {
        let font = Picker::halfblocks().font_size();
        for svg in [HOME_SVG, LIBRARY_SVG, COMMAND_PALETTE_SVG, SETTINGS_SVG] {
            let image = render_canvas(svg, font, false).expect("svg renders");
            assert_eq!(image.width(), u32::from(font.width) * 4);
            assert_eq!(image.height(), u32::from(font.height) * 2);
            assert!(
                image.to_rgba8().pixels().any(|pixel| pixel[3] > 0),
                "transparent raster"
            );
        }
    }

    fn alpha_bounds(image: &DynamicImage) -> (u32, u32, u32, u32) {
        let rgba = image.to_rgba8();
        let mut min_x = rgba.width();
        let mut min_y = rgba.height();
        let mut max_x = 0;
        let mut max_y = 0;
        for (x, y, pixel) in rgba.enumerate_pixels() {
            if pixel[3] > 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        (
            min_x,
            min_y,
            max_x.saturating_sub(min_x).saturating_add(1),
            max_y.saturating_sub(min_y).saturating_add(1),
        )
    }

    /// The raster must land on the exact protocol pixels so the
    /// documented `Resize::Fit(None)` skips its resize: one
    /// vector-to-pixels step, no resample pass, no size change.
    #[test]
    fn canvas_renders_the_icon_directly_at_protocol_resolution() {
        let font = FontSize::new(8, 16);
        let square = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1" fill="#ffffff"/></svg>"##;
        let canvas = render_canvas(square, font, false).expect("svg renders");
        assert_eq!(canvas.width(), u32::from(font.width) * 4);
        assert_eq!(canvas.height(), u32::from(font.height) * 2);
        let (_, _, width, height) = alpha_bounds(&canvas);
        let expected = u32::from(font.width) * 4 * ICON_SCALE_NUM / ICON_SCALE_DEN;
        assert_eq!(width, expected);
        assert_eq!(height, expected);
        assert!(width > canvas.width() / 2, "icon stayed at half size");
        assert!(width < canvas.width(), "icon fills the whole button");
    }

    #[test]
    fn canvas_keeps_square_artwork_square_on_a_tall_cell() {
        let font = FontSize::new(10, 18);
        let square = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><rect width="1" height="1" fill="#ffffff"/></svg>"##;
        let canvas = render_canvas(square, font, false).expect("svg renders");
        let (_, _, width, height) = alpha_bounds(&canvas);
        assert_eq!(width, height, "aspect ratio changed during rasterization");
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
