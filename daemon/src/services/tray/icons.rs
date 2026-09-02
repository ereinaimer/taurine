use std::sync::OnceLock;

const RUNNING_ICON_BYTES: &[u8] = include_bytes!("../../../../assets/icons/tray/resume/resume.png");
const PAUSED_ICON_BYTES: &[u8] = include_bytes!("../../../../assets/icons/tray/pause/pause.png");

#[cfg(target_os = "windows")]
fn get_system_tray_icon_size() -> (u32, u32) {
    // SAFETY: GetSystemMetrics is a standard read-only Win32 query for small icon display metrics.
    let w = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows_sys::Win32::UI::WindowsAndMessaging::SM_CXSMICON,
        )
    };
    let h = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows_sys::Win32::UI::WindowsAndMessaging::SM_CYSMICON,
        )
    };
    let w = if w > 0 { w as u32 } else { 32 };
    let h = if h > 0 { h as u32 } else { 32 };
    (w, h)
}

#[cfg(not(target_os = "windows"))]
fn get_system_tray_icon_size() -> (u32, u32) {
    (32, 32)
}

pub fn decode_png_to_rgba(bytes: &[u8]) -> (Vec<u8>, u32, u32) {
    let img = image::load_from_memory(bytes).expect("Failed to load embedded tray icon PNG");
    let (target_w, target_h) = get_system_tray_icon_size();

    if img.width() == target_w && img.height() == target_h {
        let rgba = img.into_rgba8();
        (rgba.into_raw(), target_w, target_h)
    } else {
        let resized = image::imageops::resize(
            &img,
            target_w,
            target_h,
            image::imageops::FilterType::Lanczos3,
        );
        (resized.into_raw(), target_w, target_h)
    }
}

pub fn running_rgba() -> &'static (Vec<u8>, u32, u32) {
    static CACHE: OnceLock<(Vec<u8>, u32, u32)> = OnceLock::new();
    CACHE.get_or_init(|| decode_png_to_rgba(RUNNING_ICON_BYTES))
}

pub fn paused_rgba() -> &'static (Vec<u8>, u32, u32) {
    static CACHE: OnceLock<(Vec<u8>, u32, u32)> = OnceLock::new();
    CACHE.get_or_init(|| decode_png_to_rgba(PAUSED_ICON_BYTES))
}

#[cfg(any(windows, target_os = "macos"))]
pub fn running_icon() -> tray_icon::Icon {
    let (rgba, width, height) = running_rgba();
    tray_icon::Icon::from_rgba(rgba.clone(), *width, *height)
        .expect("Failed to create running tray icon")
}

#[cfg(any(windows, target_os = "macos"))]
pub fn paused_icon() -> tray_icon::Icon {
    let (rgba, width, height) = paused_rgba();
    tray_icon::Icon::from_rgba(rgba.clone(), *width, *height)
        .expect("Failed to create paused tray icon")
}

#[cfg(target_os = "linux")]
pub fn rgba_to_argb32(rgba: &[u8]) -> Vec<u8> {
    let mut argb = Vec::with_capacity(rgba.len());
    for chunk in rgba.chunks_exact(4) {
        // KSNI/DBus SNI expects ARGB32 in network byte order: [A, R, G, B]
        argb.push(chunk[3]); // Alpha
        argb.push(chunk[0]); // Red
        argb.push(chunk[1]); // Green
        argb.push(chunk[2]); // Blue
    }
    argb
}

#[cfg(target_os = "linux")]
pub fn running_pixmap() -> &'static [ksni::Icon] {
    static PIXMAP: OnceLock<Vec<ksni::Icon>> = OnceLock::new();
    PIXMAP.get_or_init(|| {
        let (rgba, width, height) = running_rgba();
        let argb = rgba_to_argb32(rgba);
        vec![ksni::Icon {
            width: *width as i32,
            height: *height as i32,
            data: argb,
        }]
    })
}

#[cfg(target_os = "linux")]
pub fn paused_pixmap() -> &'static [ksni::Icon] {
    static PIXMAP: OnceLock<Vec<ksni::Icon>> = OnceLock::new();
    PIXMAP.get_or_init(|| {
        let (rgba, width, height) = paused_rgba();
        let argb = rgba_to_argb32(rgba);
        vec![ksni::Icon {
            width: *width as i32,
            height: *height as i32,
            data: argb,
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_png_to_rgba_dimensions_and_bytes() {
        let (rgba_running, w_run, h_run) = decode_png_to_rgba(RUNNING_ICON_BYTES);
        assert!(w_run > 0);
        assert!(h_run > 0);
        assert_eq!(rgba_running.len(), (w_run * h_run * 4) as usize);

        let (rgba_paused, w_pause, h_pause) = decode_png_to_rgba(PAUSED_ICON_BYTES);
        assert!(w_pause > 0);
        assert!(h_pause > 0);
        assert_eq!(rgba_paused.len(), (w_pause * h_pause * 4) as usize);
    }

    #[test]
    #[cfg(any(windows, target_os = "macos"))]
    fn test_running_and_paused_icons_load_without_panic() {
        let _run = running_icon();
        let _pause = paused_icon();
    }

    #[test]
    fn test_rgba_to_argb32_byte_ordering() {
        let rgba = [10, 20, 30, 255, 40, 50, 60, 128];
        #[cfg(target_os = "linux")]
        let argb = rgba_to_argb32(&rgba);
        #[cfg(not(target_os = "linux"))]
        let argb = {
            let mut argb = Vec::with_capacity(rgba.len());
            for chunk in rgba.chunks_exact(4) {
                argb.push(chunk[3]);
                argb.push(chunk[0]);
                argb.push(chunk[1]);
                argb.push(chunk[2]);
            }
            argb
        };

        assert_eq!(argb, vec![255, 10, 20, 30, 128, 40, 50, 60]);
    }
}
