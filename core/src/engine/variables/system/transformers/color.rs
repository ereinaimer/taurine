fn rgba_to_hsla(rgba: [f32; 4]) -> (f64, f64, f64, f64) {
    let r = rgba[0] as f64;
    let g = rgba[1] as f64;
    let b = rgba[2] as f64;
    let a = rgba[3] as f64;

    let min = r.min(g).min(b);
    let max = r.max(g).max(b);
    let delta = max - min;

    let mut h = 0.0;
    let mut s = 0.0;
    let l = (max + min) / 2.0;

    if delta != 0.0 {
        s = if l < 0.5 {
            delta / (max + min)
        } else {
            delta / (2.0 - max - min)
        };

        if max == r {
            h = (g - b) / delta + (if g < b { 6.0 } else { 0.0 });
        } else if max == g {
            h = (b - r) / delta + 2.0;
        } else {
            h = (r - g) / delta + 4.0;
        }
        h /= 6.0;
    }

    (h * 360.0, s * 100.0, l * 100.0, a)
}

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    if !args.is_empty() {
        return None;
    }

    let parsed = csscolorparser::parse(content).ok()?;
    let rgba8 = parsed.to_rgba8();
    let rgba_f32 = parsed.to_array();

    match transformer {
        "color.hex" => {
            if rgba8[3] == 255 {
                Some(format!("#{:02X}{:02X}{:02X}", rgba8[0], rgba8[1], rgba8[2]))
            } else {
                Some(format!(
                    "#{:02X}{:02X}{:02X}{:02X}",
                    rgba8[0], rgba8[1], rgba8[2], rgba8[3]
                ))
            }
        }
        "color.rgb" => {
            if rgba8[3] == 255 {
                Some(format!("rgb({}, {}, {})", rgba8[0], rgba8[1], rgba8[2]))
            } else {
                Some(format_rgba(rgba8, rgba_f32))
            }
        }
        "color.rgba" => Some(format_rgba(rgba8, rgba_f32)),
        "color.hsl" => {
            if rgba8[3] == 255 {
                Some(format_hsl(rgba_f32))
            } else {
                Some(format_hsla(rgba_f32))
            }
        }
        "color.hsla" => Some(format_hsla(rgba_f32)),
        _ => None,
    }
}

fn format_rgba(rgba8: [u8; 4], rgba_f32: [f32; 4]) -> String {
    let alpha_str = format!("{:.3}", rgba_f32[3]);
    let alpha_trimmed = alpha_str.trim_end_matches('0').trim_end_matches('.');
    let alpha_val = if alpha_trimmed.is_empty() {
        "0"
    } else {
        alpha_trimmed
    };
    format!(
        "rgba({}, {}, {}, {})",
        rgba8[0], rgba8[1], rgba8[2], alpha_val
    )
}

fn format_hsl(rgba_f32: [f32; 4]) -> String {
    let (h, s, l, _) = rgba_to_hsla(rgba_f32);
    format!(
        "hsl({}, {}%, {}%)",
        h.round() as i32,
        s.round() as i32,
        l.round() as i32
    )
}

fn format_hsla(rgba_f32: [f32; 4]) -> String {
    let (h, s, l, a) = rgba_to_hsla(rgba_f32);
    let alpha_str = format!("{:.3}", a);
    let alpha_trimmed = alpha_str.trim_end_matches('0').trim_end_matches('.');
    let alpha_val = if alpha_trimmed.is_empty() {
        "0"
    } else {
        alpha_trimmed
    };
    format!(
        "hsla({}, {}%, {}%, {})",
        h.round() as i32,
        s.round() as i32,
        l.round() as i32,
        alpha_val
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_conversions() {
        assert_eq!(
            apply("color.hex", &[], "#ff0000"),
            Some("#FF0000".to_string())
        );
        assert_eq!(
            apply("color.rgb", &[], "#ff0000"),
            Some("rgb(255, 0, 0)".to_string())
        );
        assert_eq!(
            apply("color.rgba", &[], "red"),
            Some("rgba(255, 0, 0, 1)".to_string())
        );
        assert_eq!(
            apply("color.hsl", &[], "#00ff00"),
            Some("hsl(120, 100%, 50%)".to_string())
        );
        assert_eq!(
            apply("color.hsla", &[], "rgba(255,255,255,0.5)"),
            Some("hsla(0, 0%, 100%, 0.5)".to_string())
        );
        assert_eq!(apply("color.hex", &[], "invalid-color"), None);
    }
}
