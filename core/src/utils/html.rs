use regex::Regex;
use std::sync::OnceLock;

/// Checks if the text contains any HTML tags.
pub fn has_html_tags(text: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"</?[a-zA-Z!]").unwrap());
    re.is_match(text)
}

/// Strips HTML tags and decodes basic entities, formatting newlines for `<br>` and `</p>`.
pub fn strip_html(html: &str) -> String {
    let mut plain = String::new();
    let mut in_tag = false;
    let mut tag_name = String::new();

    for c in html.chars() {
        if c == '<' {
            in_tag = true;
            tag_name.clear();
        } else if c == '>' {
            in_tag = false;
            let tag_lower = tag_name.to_lowercase();
            if tag_lower == "br" || tag_lower == "br/" || tag_lower == "/p" || tag_lower == "/div" {
                plain.push('\n');
            }
        } else if in_tag {
            if !c.is_whitespace() {
                tag_name.push(c);
            }
        } else {
            plain.push(c);
        }
    }

    plain
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_html_tags() {
        assert!(has_html_tags("<p>hello</p>"));
        assert!(has_html_tags("hello <br> world"));
        assert!(has_html_tags("hello </br>"));
        assert!(!has_html_tags("hello < world"));
        assert!(!has_html_tags("hello > world"));
        assert!(!has_html_tags("hello <3 world"));
    }

    #[test]
    fn test_strip_html() {
        assert_eq!(strip_html("<p>hello</p> world"), "hello\n world");
        assert_eq!(strip_html("hello<br/>world"), "hello\nworld");
        assert_eq!(strip_html("hello &amp; world"), "hello & world");
        assert_eq!(strip_html("hello&nbsp;world"), "hello world");
    }
}
