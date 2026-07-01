use base64::{Engine as _, engine::general_purpose::STANDARD};

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    if !args.is_empty() {
        return None;
    }

    match transformer {
        "url.encode" => Some(urlencode_string(content)),
        "url.decode" => urldecode_string(content),
        "url.clean" => Some(url_clean(content)),
        "base64.encode" => Some(STANDARD.encode(content)),
        "base64.decode" => STANDARD
            .decode(content.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok()),
        _ => None,
    }
}

fn url_clean(content: &str) -> String {
    let content = content.trim();
    if let Some(idx) = content.find('?') {
        content[..idx].to_string()
    } else {
        content.to_string()
    }
}

fn urlencode_string(content: &str) -> String {
    let mut out = String::with_capacity(content.len());

    for byte in content.as_bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }

    out
}

fn urldecode_string(content: &str) -> Option<String> {
    let bytes = content.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut idx = 0;

    while idx < bytes.len() {
        match bytes[idx] {
            b'%' if idx + 2 < bytes.len() => {
                let high = hex_value(bytes[idx + 1])?;
                let low = hex_value(bytes[idx + 2])?;
                decoded.push((high << 4) | low);
                idx += 3;
            }
            b'%' => return None,
            b'+' => {
                decoded.push(b' ');
                idx += 1;
            }
            byte => {
                decoded.push(byte);
                idx += 1;
            }
        }
    }

    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoding_transformers() {
        assert_eq!(
            apply("url.encode", &[], "hello world!"),
            Some("hello%20world%21".to_string())
        );
        assert_eq!(
            apply("url.decode", &[], "hello%20world%21"),
            Some("hello world!".to_string())
        );
        assert_eq!(
            apply(
                "url.clean",
                &[],
                "https://google.com/search?q=rust&utm_source=facebook"
            ),
            Some("https://google.com/search".to_string())
        );
        assert_eq!(
            apply("url.clean", &[], "https://google.com/search"),
            Some("https://google.com/search".to_string())
        );
        assert_eq!(
            apply("url.clean", &[], "  https://google.com/search?q=123   "),
            Some("https://google.com/search".to_string())
        );
        assert_eq!(
            apply("base64.encode", &[], "hello"),
            Some("aGVsbG8=".to_string())
        );
        assert_eq!(
            apply("base64.decode", &[], "aGVsbG8="),
            Some("hello".to_string())
        );
        assert_eq!(
            apply("base64.decode", &[], "  aGVsbG8=\n"),
            Some("hello".to_string())
        );
        assert_eq!(apply("url.decode", &[], "%ZZ"), None);
    }
}
