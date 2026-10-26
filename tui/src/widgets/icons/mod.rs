//! Shared icon rendering: hardcoded SVGs rasterized once at startup and
//! a text fallback for terminals without a graphics protocol.

mod svg;

use image::{DynamicImage, RgbaImage};
use ratatui::{
    Frame,
    layout::{Alignment, Rect, Size},
    style::{Color, Style},
    widgets::Paragraph,
};
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::{FontSize, Image, Resize};
use taurine_core::db::TargetOs;

use crate::theme::Theme;

/// Muted variant factor, matching the theme's muted text on white.
const MUTED: f32 = 0.5;

/// Text replacements keep the UI readable on strict TTYs.
pub(crate) const CHEVRON_DOWN: &str = "▾";
pub(crate) const CHEVRON_UP: &str = "▴";
pub(crate) const CLOSE_ICON: &str = "×";
pub(crate) const ADD_ICON: &str = "+";
pub(crate) const ADD_CHIP: &str = " + ";
pub(crate) const SEARCH_FALLBACK: &str = "[S]";
/// Widest platform fallback ("[Win]", "[Mac]", "[iOS]").
pub(crate) const PLATFORM_ICON_WIDTH: u16 = 5;

/// Muted and bright protocols for the navigation rail; the button itself
/// never paints a fill. Halfblocks drop alpha, so only that protocol gets
/// the rail color merged into transparent pixels.
pub(crate) struct IconVariants {
    pub(crate) idle: Protocol,
    pub(crate) bright: Protocol,
}

/// One cached inline icon.
struct InlineIcon(Protocol);

impl InlineIcon {
    fn protocol(&self) -> &Protocol {
        &self.0
    }
}

pub(crate) struct AppIcons {
    pub(crate) home: IconVariants,
    pub(crate) library: IconVariants,
    pub(crate) command_palette: IconVariants,
    pub(crate) settings: IconVariants,
    search: Option<InlineIcon>,
    windows: Option<InlineIcon>,
    macos: Option<InlineIcon>,
    linux: Option<InlineIcon>,
    android: Option<InlineIcon>,
    ios: Option<InlineIcon>,
    all: Option<InlineIcon>,
}

impl AppIcons {
    pub(crate) fn load(picker: &Picker, theme: &Theme) -> Result<Self, String> {
        let halfblocks = picker.protocol_type() == ProtocolType::Halfblocks;
        let rail_bg = rgb(theme.background);
        let inline = !halfblocks;
        Ok(Self {
            home: IconVariants::load(picker, svg::HOME_SVG, halfblocks, rail_bg)?,
            library: IconVariants::load(picker, svg::LIBRARY_SVG, halfblocks, rail_bg)?,
            command_palette: IconVariants::load(
                picker,
                svg::COMMAND_PALETTE_SVG,
                halfblocks,
                rail_bg,
            )?,
            settings: IconVariants::load(picker, svg::SETTINGS_SVG, halfblocks, rail_bg)?,
            search: inline_icon(picker, svg::SEARCH_SVG, inline)?,
            windows: inline_icon(picker, svg::WINDOWS_SVG, inline)?,
            macos: inline_icon(picker, svg::MACOS_SVG, inline)?,
            linux: inline_icon(picker, svg::LINUX_SVG, inline)?,
            android: inline_icon(picker, svg::ANDROID_SVG, inline)?,
            ios: inline_icon(picker, svg::IOS_SVG, inline)?,
            all: inline_icon(picker, svg::ALL_SVG, inline)?,
        })
    }

    pub(crate) fn search(&self) -> Option<&Protocol> {
        self.search.as_ref().map(InlineIcon::protocol)
    }

    pub(crate) fn platform(&self, platform: PlatformIcon) -> Option<&Protocol> {
        let icon = match platform {
            PlatformIcon::All => self.all.as_ref(),
            PlatformIcon::Windows => self.windows.as_ref(),
            PlatformIcon::MacOs => self.macos.as_ref(),
            PlatformIcon::Linux => self.linux.as_ref(),
            PlatformIcon::Android => self.android.as_ref(),
            PlatformIcon::Ios => self.ios.as_ref(),
        };
        icon.map(InlineIcon::protocol)
    }
}

impl IconVariants {
    fn load(picker: &Picker, svg: &str, opaque: bool, background: [u8; 3]) -> Result<Self, String> {
        let font = picker.font_size();
        let bright = render_canvas(svg, font, 4, 2, 2, 1, opaque)?;
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
            idle: protocol(picker, idle, Size::new(4, 2))?,
            bright: protocol(picker, bright, Size::new(4, 2))?,
        })
    }
}

fn inline_icon(picker: &Picker, svg: &str, enabled: bool) -> Result<Option<InlineIcon>, String> {
    if !enabled {
        return Ok(None);
    }
    let image = render_canvas(svg, picker.font_size(), 2, 1, 2, 1, false)?;
    Ok(Some(InlineIcon(protocol(picker, image, Size::new(2, 1))?)))
}

fn protocol(picker: &Picker, image: DynamicImage, size: Size) -> Result<Protocol, String> {
    picker
        .new_protocol(image, size, Resize::Fit(None))
        .map_err(|error| error.to_string())
}

/// Rasterize the SVG once into the protocol's exact cell canvas. Graphics
/// protocols get the artwork at `glyph_cols` x `glyph_rows` cells, centered;
/// the half-block fallback fills the canvas to keep any detail.
fn render_canvas(
    svg: &str,
    font: FontSize,
    canvas_cols: u16,
    canvas_rows: u16,
    glyph_cols: u16,
    glyph_rows: u16,
    opaque: bool,
) -> Result<DynamicImage, String> {
    let canvas_w = u32::from(font.width) * u32::from(canvas_cols);
    let canvas_h = u32::from(font.height) * u32::from(canvas_rows);
    if canvas_w == 0 || canvas_h == 0 {
        return Err("terminal reports a zero-size cell".to_string());
    }
    let (glyph_w, glyph_h) = if opaque {
        (canvas_w, canvas_h)
    } else {
        (
            u32::from(font.width) * u32::from(glyph_cols),
            u32::from(font.height) * u32::from(glyph_rows),
        )
    };
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(svg, &options).map_err(|error| error.to_string())?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return Err("icon SVG has no size".to_string());
    }
    let scale = (glyph_w as f32 / size.width()).min(glyph_h as f32 / size.height());
    let tx = (canvas_w as f32 - size.width() * scale) / 2.0;
    let ty = (canvas_h as f32 - size.height() * scale) / 2.0;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(canvas_w, canvas_h)
        .ok_or_else(|| "failed to allocate the icon raster".to_string())?;
    let transform = resvg::tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, tx, ty);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let image = RgbaImage::from_raw(canvas_w, canvas_h, pixmap.take())
        .ok_or_else(|| "failed to wrap the icon raster".to_string())?;
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

/// Draw a cached icon centered in `area`; without a protocol, render the
/// ASCII fallback so strict TTYs stay usable.
pub(crate) fn render_icon(
    frame: &mut Frame,
    area: Rect,
    protocol: Option<&Protocol>,
    fallback: &str,
    style: Style,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if let Some(protocol) = protocol {
        let size = protocol.size();
        let width = size.width.min(area.width);
        let height = size.height.min(area.height);
        let rect = Rect {
            x: area.x.saturating_add(area.width.saturating_sub(width) / 2),
            y: area
                .y
                .saturating_add(area.height.saturating_sub(height) / 2),
            width,
            height,
        };
        frame.render_widget(Image::new(protocol).allow_clipping(true), rect);
        return;
    }
    let fallback_area = Rect {
        x: area.x,
        y: area.y.saturating_add(area.height.saturating_sub(1) / 2),
        width: area.width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(fallback)
            .alignment(Alignment::Center)
            .style(style),
        fallback_area,
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlatformIcon {
    All,
    Windows,
    MacOs,
    Linux,
    Android,
    Ios,
}

impl PlatformIcon {
    pub(crate) fn for_target(target_os: &str) -> Self {
        match TargetOs::parse_str(target_os) {
            Some(TargetOs::Windows) => Self::Windows,
            Some(TargetOs::MacOs) => Self::MacOs,
            Some(TargetOs::Linux) => Self::Linux,
            Some(TargetOs::Android) => Self::Android,
            Some(TargetOs::Ios) => Self::Ios,
            _ => Self::All,
        }
    }

    pub(crate) const fn fallback(self) -> &'static str {
        match self {
            Self::All => "[All]",
            Self::Windows => "[Win]",
            Self::MacOs => "[Mac]",
            Self::Linux => "[Lin]",
            Self::Android => "[And]",
            Self::Ios => "[iOS]",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::default::DARK_THEME;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn every_svg_rasterizes_with_visible_pixels() {
        let font = Picker::halfblocks().font_size();
        let assets = [
            (svg::HOME_SVG, 4, 2, 2, 1),
            (svg::LIBRARY_SVG, 4, 2, 2, 1),
            (svg::COMMAND_PALETTE_SVG, 4, 2, 2, 1),
            (svg::SETTINGS_SVG, 4, 2, 2, 1),
            (svg::SEARCH_SVG, 2, 1, 2, 1),
            (svg::WINDOWS_SVG, 2, 1, 2, 1),
            (svg::MACOS_SVG, 2, 1, 2, 1),
            (svg::LINUX_SVG, 2, 1, 2, 1),
            (svg::ANDROID_SVG, 2, 1, 2, 1),
            (svg::IOS_SVG, 2, 1, 2, 1),
            (svg::ALL_SVG, 2, 1, 2, 1),
        ];
        for (svg, cols, rows, glyph_cols, glyph_rows) in assets {
            let image = render_canvas(svg, font, cols, rows, glyph_cols, glyph_rows, false)
                .expect("svg renders");
            assert_eq!(image.width(), u32::from(font.width) * u32::from(cols));
            assert_eq!(image.height(), u32::from(font.height) * u32::from(rows));
            assert!(
                image.to_rgba8().pixels().any(|pixel| pixel[3] > 0),
                "transparent raster"
            );
        }
    }

    #[test]
    fn rail_canvas_centers_the_half_size_icon_without_resampling() {
        let font = Picker::halfblocks().font_size();
        let canvas = render_canvas(svg::HOME_SVG, font, 4, 2, 2, 1, false).expect("svg renders");
        let rgba = canvas.to_rgba8();
        assert_eq!(rgba.get_pixel(0, 0)[3], 0, "corner is transparent");
        assert!(
            rgba.get_pixel(u32::from(font.width) * 2, u32::from(font.height))[3] > 0,
            "center holds the artwork"
        );
    }

    #[test]
    fn halfblocks_cache_rail_icons_and_uses_text_for_inline_icons() {
        let icons = AppIcons::load(&Picker::halfblocks(), &DARK_THEME).expect("icons load");
        for icon in [
            &icons.home,
            &icons.library,
            &icons.command_palette,
            &icons.settings,
        ] {
            assert_eq!(icon.idle.size(), Size::new(4, 2));
            assert_eq!(icon.bright.size(), Size::new(4, 2));
        }
        assert!(icons.search().is_none());
        for platform in [
            PlatformIcon::All,
            PlatformIcon::Windows,
            PlatformIcon::MacOs,
            PlatformIcon::Linux,
            PlatformIcon::Android,
            PlatformIcon::Ios,
        ] {
            assert!(icons.platform(platform).is_none(), "{platform:?}");
        }
    }

    #[test]
    fn graphics_cache_inline_icons_at_two_by_one_cells() {
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Sixel);
        let icons = AppIcons::load(&picker, &DARK_THEME).expect("icons load");
        assert_eq!(icons.search().expect("search icon").size(), Size::new(2, 1));
        for platform in [
            PlatformIcon::All,
            PlatformIcon::Windows,
            PlatformIcon::MacOs,
            PlatformIcon::Linux,
            PlatformIcon::Android,
            PlatformIcon::Ios,
        ] {
            assert_eq!(
                icons.platform(platform).expect("platform icon").size(),
                Size::new(2, 1),
                "{platform:?}"
            );
        }
    }

    #[test]
    fn missing_protocol_renders_the_ascii_fallback() {
        let area = Rect::new(0, 0, 5, 1);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_icon(
                    frame,
                    area,
                    None,
                    PlatformIcon::Windows.fallback(),
                    Style::default(),
                );
            })
            .expect("test draw");
        let buffer = terminal.backend().buffer();
        let row: String = (0..area.width).map(|x| buffer[(x, 0)].symbol()).collect();
        assert_eq!(row, "[Win]");
    }

    #[test]
    fn platform_fallbacks_fit_the_reserved_slot() {
        for platform in [
            PlatformIcon::All,
            PlatformIcon::Windows,
            PlatformIcon::MacOs,
            PlatformIcon::Linux,
            PlatformIcon::Android,
            PlatformIcon::Ios,
        ] {
            assert!(
                platform.fallback().chars().count() as u16 <= PLATFORM_ICON_WIDTH,
                "{platform:?}"
            );
        }
    }
}
