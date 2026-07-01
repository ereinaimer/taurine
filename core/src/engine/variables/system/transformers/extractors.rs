use regex::Regex;
use scraper::{Html, Selector};
use serde_json::Value as JsonValue;
use serde_yml::Value as YamlValue;
use std::sync::LazyLock;
use toml::Value as TomlValue;

use super::strip_argument_quotes;

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    match transformer {
        "json" => apply_json(args, content),
        "html" | "xml" => apply_html_xml(args, content),
        "toml" => apply_toml(args, content),
        "yaml" => apply_yaml(args, content),
        "regexmatch" => apply_regexmatch(args, content),
        "ext.url" => Some(extract_url(content)),
        "ext.email" => Some(extract_email(content)),
        "ext.phone" => Some(extract_phone(content)),
        "ext.mention" => Some(extract_mention(content)),
        "ext.hashtag" => Some(extract_hashtag(content)),
        "ext.ip" => Some(extract_ip(content)),
        "ext.mac" => Some(extract_mac(content)),
        "ext.path" => Some(extract_path(content)),
        "ext.path.filename" => Some(extract_path_filename(content)),
        "ext.path.dir" => Some(extract_path_dir(content)),
        "ext.jwt" => Some(extract_jwt(content)),
        "ext.semver" => Some(extract_semver(content)),
        "ext.mdcode" => Some(extract_mdcode(content)),
        "ext.mdtable" => Some(extract_mdtable(content)),
        "ext.mdlist" => Some(extract_mdlist(content)),
        _ => None,
    }
}

fn apply_json(args: &[&str], content: &str) -> Option<String> {
    let path = strip_argument_quotes(args.first()?);
    let mut current: JsonValue = serde_json::from_str(content).ok()?;

    for segment in path.split('.') {
        if let Ok(idx) = segment.parse::<usize>() {
            current = current.get(idx)?.clone();
        } else {
            current = current.get(segment)?.clone();
        }
    }

    match current {
        JsonValue::String(s) => Some(s),
        JsonValue::Null => None,
        other => Some(other.to_string()),
    }
}

fn apply_html_xml(args: &[&str], content: &str) -> Option<String> {
    let selector_str = strip_argument_quotes(args.first()?);
    let selector = Selector::parse(selector_str).ok()?;
    let document = Html::parse_document(content);

    document
        .select(&selector)
        .next()
        .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
}

fn apply_toml(args: &[&str], content: &str) -> Option<String> {
    let path = strip_argument_quotes(args.first()?);
    let mut current: TomlValue = toml::from_str(content).ok()?;

    for segment in path.split('.') {
        if let Ok(idx) = segment.parse::<usize>() {
            current = current.get(idx)?.clone();
        } else {
            current = current.get(segment)?.clone();
        }
    }

    match current {
        TomlValue::String(s) => Some(s),
        TomlValue::Integer(i) => Some(i.to_string()),
        TomlValue::Float(f) => Some(f.to_string()),
        TomlValue::Boolean(b) => Some(b.to_string()),
        TomlValue::Datetime(d) => Some(d.to_string()),
        _ => None,
    }
}

fn apply_yaml(args: &[&str], content: &str) -> Option<String> {
    let path = strip_argument_quotes(args.first()?);
    let mut current: YamlValue = serde_yml::from_str(content).ok()?;

    for segment in path.split('.') {
        if let Ok(idx) = segment.parse::<usize>() {
            current = current.get(idx)?.clone();
        } else {
            current = current.get(segment)?.clone();
        }
    }

    match current {
        YamlValue::String(s) => Some(s.clone()),
        YamlValue::Number(n) => Some(n.to_string()),
        YamlValue::Bool(b) => Some(b.to_string()),
        YamlValue::Null => None,
        other => serde_yml::to_string(&other)
            .ok()
            .map(|s| s.trim().to_string()),
    }
}

fn apply_regexmatch(args: &[&str], content: &str) -> Option<String> {
    let pattern = strip_argument_quotes(args.first()?);
    let group_index: usize = args
        .get(1)
        .and_then(|g| strip_argument_quotes(g).parse().ok())
        .unwrap_or(0);

    let re = Regex::new(pattern).ok()?;
    let caps = re.captures(content)?;

    caps.get(group_index).map(|m| m.as_str().to_string())
}

static URL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https?://[^\s<>"']+"#).expect("valid URL regex"));
static EMAIL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b").expect("valid email regex")
});
static PHONE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    // No leading \b: \b would exclude + and ( from matches. Instead, a
    // post-match character check in extract_phone rejects matches where the
    // preceding byte is a digit, achieving the same protection.
    Regex::new(r"(?:\+?\d{1,3}[-.\s]?)?\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}\b")
        .expect("valid phone regex")
});
static MENTION_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\B@[\w.-]+\b").expect("valid mention regex"));
static HASHTAG_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\B#[a-zA-Z_][\w-]*\b").expect("valid hashtag regex"));
static IP_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b|\b(?:[A-Fa-f0-9]{1,4}:){7}[A-Fa-f0-9]{1,4}\b").expect("valid IP regex")
});
static MAC_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b").expect("valid MAC regex")
});
static PATH_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\b[a-zA-Z]:\\[\w.-]+(?:\\[\w.-]+)+|/[\w.-]+(?:/[\w.-]+)+|\b[\w.-]+(?:/[\w.-]+)+|\b[\w.-]+(?:\\[\w.-]+)+"#).expect("valid path regex")
});
static JWT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b").expect("valid JWT regex")
});
static SEMVER_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bv?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?\b").expect("valid semver regex")
});
static MDCODE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)```(?:[a-zA-Z0-9+-]+)?\n(.*?)\n```").expect("valid mdcode regex")
});

fn trim_trailing_punctuation(match_text: &str) -> String {
    match_text
        .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']'])
        .to_string()
}

fn extract_url(content: &str) -> String {
    URL_REGEX
        .find_iter(content)
        .map(|c| trim_trailing_punctuation(c.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}
fn extract_email(content: &str) -> String {
    EMAIL_REGEX
        .find_iter(content)
        .map(|c| c.as_str().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
fn extract_phone(content: &str) -> String {
    // Post-filter: reject any match where the character immediately before the
    // match start is an ASCII digit, preventing false positives from embedded
    // digit runs (e.g. "ID12345678901234").  Leading + and ( are non-digit so
    // they are correctly included in the match and not filtered out.
    let bytes = content.as_bytes();
    PHONE_REGEX
        .find_iter(content)
        .filter(|m| {
            let start = m.start();
            if start == 0 {
                return true;
            }
            // Walk backwards over any leading non-digit chars (+, whitespace…)
            // to find the true first digit of this match, then check what
            // precedes the whole token.
            let preceding = bytes[start - 1];
            !preceding.is_ascii_digit()
        })
        .map(|m| m.as_str().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
fn extract_mention(content: &str) -> String {
    MENTION_REGEX
        .find_iter(content)
        .map(|c| c.as_str().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
fn extract_hashtag(content: &str) -> String {
    HASHTAG_REGEX
        .find_iter(content)
        .map(|c| c.as_str().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
fn extract_ip(content: &str) -> String {
    IP_REGEX
        .find_iter(content)
        .map(|c| c.as_str().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
fn extract_mac(content: &str) -> String {
    MAC_REGEX
        .find_iter(content)
        .map(|c| c.as_str().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
fn extract_path(content: &str) -> String {
    PATH_REGEX
        .find_iter(content)
        .map(|c| c.as_str().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
fn extract_path_filename(content: &str) -> String {
    // Per spec: extract filenames by running the path regex then pulling the
    // file_name() component via std::path::Path, falling back to matching
    // standalone filenames that appear outside of a path context.
    let mut results: Vec<String> = Vec::new();
    let mut covered_ranges: Vec<std::ops::Range<usize>> = Vec::new();

    // First pass: paths found by PATH_REGEX – extract filename component.
    for m in PATH_REGEX.find_iter(content) {
        let path = std::path::Path::new(m.as_str());
        if let Some(fname) = path.file_name() {
            let fname_str = fname.to_string_lossy().into_owned();
            // Only emit if the filename itself has an extension (i.e., it is a
            // file, not a bare directory path).
            if path.extension().is_some() {
                results.push(fname_str);
            }
        }
        covered_ranges.push(m.range());
    }

    // Second pass: standalone filenames (with extension) not already covered
    // by a path match.  Uses a lightweight ad-hoc scan so we still honour the
    // PATH_REGEX+Path requirement as the primary mechanism.
    static STANDALONE_FILENAME_REGEX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\b[\w-]+\.[a-zA-Z0-9]{1,8}\b").expect("valid standalone filename regex")
    });
    for m in STANDALONE_FILENAME_REGEX.find_iter(content) {
        let range = m.range();
        let already_covered = covered_ranges
            .iter()
            .any(|r| r.start <= range.start && range.end <= r.end);
        if !already_covered {
            results.push(m.as_str().to_string());
        }
    }

    results.join("\n")
}
fn extract_path_dir(content: &str) -> String {
    PATH_REGEX
        .find_iter(content)
        .filter_map(|m| {
            let path_str = m.as_str();
            let path = std::path::Path::new(path_str);
            if path.extension().is_some() {
                path.parent().map(|p| p.to_string_lossy().into_owned())
            } else {
                Some(path_str.to_string())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn extract_jwt(content: &str) -> String {
    JWT_REGEX
        .find_iter(content)
        .map(|c| c.as_str().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
fn extract_semver(content: &str) -> String {
    SEMVER_REGEX
        .find_iter(content)
        .map(|c| c.as_str().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
fn extract_mdcode(content: &str) -> String {
    MDCODE_REGEX
        .captures_iter(content)
        .filter_map(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
        .collect::<Vec<_>>()
        .join("\n\n")
}
fn extract_mdtable(content: &str) -> String {
    let mut result = Vec::new();
    let mut current_table = Vec::new();
    let mut has_separator = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            current_table.push(line);
            if trimmed.replace(" ", "").contains("|---|")
                || trimmed.replace(" ", "").contains("|:---|")
                || trimmed.replace(" ", "").contains("|---:|")
                || trimmed.replace(" ", "").contains("|:---:|")
            {
                has_separator = true;
            }
        } else {
            if !current_table.is_empty() {
                if has_separator {
                    result.push(current_table.join("\n"));
                }
                current_table.clear();
                has_separator = false;
            }
        }
    }
    if !current_table.is_empty() && has_separator {
        result.push(current_table.join("\n"));
    }
    result.join("\n\n")
}
fn extract_mdlist(content: &str) -> String {
    let mut result = Vec::new();
    let mut current_list = Vec::new();

    let is_list_item = |line: &str| -> bool {
        let trimmed = line.trim_start();
        trimmed.starts_with("* ")
            || trimmed.starts_with("- ")
            || trimmed.starts_with("+ ")
            || (trimmed.contains(". ")
                && trimmed.chars().take_while(|c| c.is_ascii_digit()).count() > 0
                && trimmed[trimmed.chars().take_while(|c| c.is_ascii_digit()).count()..]
                    .starts_with(". "))
    };

    for line in content.lines() {
        if is_list_item(line) {
            current_list.push(line);
        } else if !line.trim().is_empty() {
            if !current_list.is_empty() && (line.starts_with("  ") || line.starts_with('\t')) {
                current_list.push(line);
            } else {
                if !current_list.is_empty() {
                    result.push(current_list.join("\n"));
                    current_list.clear();
                }
            }
        } else {
            if !current_list.is_empty() {
                result.push(current_list.join("\n"));
                current_list.clear();
            }
        }
    }
    if !current_list.is_empty() {
        result.push(current_list.join("\n"));
    }
    result.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_json() {
        let json = r#"{"user": {"name": "Alice", "hobbies": ["reading", "gaming"]}}"#;
        assert_eq!(apply_json(&["user.name"], json), Some("Alice".to_string()));
        assert_eq!(
            apply_json(&["user.hobbies.1"], json),
            Some("gaming".to_string())
        );
        assert_eq!(apply_json(&["invalid.path"], json), None);
    }

    #[test]
    fn test_apply_html() {
        let html = r#"<html><body><div class="price"><span>$99.99</span></div></body></html>"#;
        assert_eq!(
            apply_html_xml(&[".price > span"], html),
            Some("$99.99".to_string())
        );
    }

    #[test]
    fn test_apply_xml() {
        let xml = r#"<?xml version="1.0"?><bookstore><book><title>Rust Programming</title></book></bookstore>"#;
        assert_eq!(
            apply_html_xml(&["bookstore > book > title"], xml),
            Some("Rust Programming".to_string())
        );
    }

    #[test]
    fn test_apply_toml() {
        let toml_str = r#"
        [package]
        name = "taurine"
        authors = ["erein"]
        "#;
        assert_eq!(
            apply_toml(&["package.name"], toml_str),
            Some("taurine".to_string())
        );
        assert_eq!(
            apply_toml(&["package.authors.0"], toml_str),
            Some("erein".to_string())
        );
    }

    #[test]
    fn test_apply_yaml() {
        let yaml_str = "
services:
  db:
    image: postgres:15
        ";
        assert_eq!(
            apply_yaml(&["services.db.image"], yaml_str),
            Some("postgres:15".to_string())
        );
    }

    #[test]
    fn test_apply_regexmatch() {
        let text = "Order ID: #12345 (Completed)";
        assert_eq!(
            apply_regexmatch(&["#([0-9]+)", "1"], text),
            Some("12345".to_string())
        );
        assert_eq!(
            apply_regexmatch(&["#([0-9]+)"], text),
            Some("#12345".to_string()) // Default to group 0 (full match)
        );
        assert_eq!(apply_regexmatch(&["invalid_regex["], text), None);
    }

    #[test]
    fn test_apply_ext_url() {
        assert_eq!(
            apply(
                "ext.url",
                &[],
                "visit https://taurine.in and http://localhost:8080/!"
            ),
            Some("https://taurine.in\nhttp://localhost:8080/".to_string())
        );
        // No URLs: should return empty string
        assert_eq!(apply("ext.url", &[], "no links here"), Some("".to_string()));
    }

    #[test]
    fn test_apply_ext_email() {
        assert_eq!(
            apply(
                "ext.email",
                &[],
                "contact admin@example.com or user@test.co.uk"
            ),
            Some("admin@example.com\nuser@test.co.uk".to_string())
        );
        // Invalid email (no TLD) should not match
        assert_eq!(
            apply("ext.email", &[], "not-an-email@"),
            Some("".to_string())
        );
    }

    #[test]
    fn test_apply_ext_phone() {
        assert_eq!(
            apply("ext.phone", &[], "Call +1-800-555-0199 or (555) 123-4567"),
            Some("+1-800-555-0199\n(555) 123-4567".to_string())
        );
        // Short digit sequence must NOT match due to leading \b
        assert_eq!(apply("ext.phone", &[], "code 123"), Some("".to_string()));
        // Digits embedded in a longer run should not produce a spurious match
        assert_eq!(
            apply("ext.phone", &[], "ID12345678901234"),
            Some("".to_string())
        );
    }

    #[test]
    fn test_apply_ext_mention() {
        assert_eq!(
            apply("ext.mention", &[], "Hello @alice and @bob-smith!"),
            Some("@alice\n@bob-smith".to_string())
        );
        // Lone @ is not a mention
        assert_eq!(
            apply("ext.mention", &[], "email@ not a mention"),
            Some("".to_string())
        );
    }

    #[test]
    fn test_apply_ext_hashtag() {
        assert_eq!(
            apply(
                "ext.hashtag",
                &[],
                "Learning #rust and #text-expansion! Not #123"
            ),
            Some("#rust\n#text-expansion".to_string())
        );
        // Pure-digit hashtag must not match
        assert_eq!(
            apply("ext.hashtag", &[], "#42 is not a hashtag"),
            Some("".to_string())
        );
    }

    #[test]
    fn test_apply_ext_ip() {
        assert_eq!(
            apply(
                "ext.ip",
                &[],
                "IPv4: 192.168.1.1, IPv6: 2001:0db8:85a3:0000:0000:8a2e:0370:7334"
            ),
            Some("192.168.1.1\n2001:0db8:85a3:0000:0000:8a2e:0370:7334".to_string())
        );
        // Out-of-range octet: 999.999.999.999 must NOT match
        assert_eq!(
            apply("ext.ip", &[], "999.999.999.999"),
            Some("".to_string())
        );
        // No IPs at all
        assert_eq!(apply("ext.ip", &[], "nothing here"), Some("".to_string()));
    }

    #[test]
    fn test_apply_ext_mac() {
        assert_eq!(
            apply("ext.mac", &[], "Device MAC: 00:1A:2B:3C:4D:5E"),
            Some("00:1A:2B:3C:4D:5E".to_string())
        );
        // Dash-separated MAC
        assert_eq!(
            apply("ext.mac", &[], "MAC: AA-BB-CC-DD-EE-FF"),
            Some("AA-BB-CC-DD-EE-FF".to_string())
        );
        // Too few groups must NOT match
        assert_eq!(
            apply("ext.mac", &[], "00:1A:2B:3C:4D"),
            Some("".to_string())
        );
    }

    #[test]
    fn test_apply_ext_path() {
        let text = "Logs at /var/log/syslog and C:\\Windows\\System32\\cmd.exe";
        assert_eq!(
            apply("ext.path", &[], text),
            Some("/var/log/syslog\nC:\\Windows\\System32\\cmd.exe".to_string())
        );
        // No paths
        assert_eq!(apply("ext.path", &[], "just words"), Some("".to_string()));
    }

    #[test]
    fn test_apply_ext_path_filename() {
        // Standalone filenames
        assert_eq!(
            apply(
                "ext.path.filename",
                &[],
                "Check package.json and src/main.rs"
            ),
            Some("main.rs\npackage.json".to_string()).or(Some("package.json\nmain.rs".to_string()))
        );
        // Filename extracted from a full path via PATH_REGEX + std::path::Path
        assert_eq!(
            apply("ext.path.filename", &[], "/usr/local/bin/taurine.exe"),
            Some("taurine.exe".to_string())
        );
        // Directory-only path should produce no filename (no extension)
        assert_eq!(
            apply("ext.path.filename", &[], "/usr/local/bin"),
            Some("".to_string())
        );
    }

    #[test]
    fn test_apply_ext_path_dir() {
        assert_eq!(
            apply(
                "ext.path.dir",
                &[],
                "File /usr/local/bin/taurine.exe and folder /var/log/"
            ),
            Some("/usr/local/bin\n/var/log".to_string())
        );
    }

    #[test]
    fn test_apply_ext_jwt() {
        assert_eq!(
            apply("ext.jwt", &[], "Token: eyJhbGci.eyJzdWIi.SflKxwRJS"),
            Some("eyJhbGci.eyJzdWIi.SflKxwRJS".to_string())
        );
        // Must not match a non-JWT token that does not start with eyJ
        assert_eq!(apply("ext.jwt", &[], "abc.def.ghi"), Some("".to_string()));
    }

    #[test]
    fn test_apply_ext_semver() {
        assert_eq!(
            apply("ext.semver", &[], "v1.2.3 and 0.4.0-alpha.1"),
            Some("v1.2.3\n0.4.0-alpha.1".to_string())
        );
        // Partial version (only major.minor) must NOT match
        assert_eq!(
            apply("ext.semver", &[], "version 1.2 is not semver"),
            Some("".to_string())
        );
        // Pre-release and build metadata
        assert_eq!(
            apply("ext.semver", &[], "1.0.0-beta+exp.sha.5114f85"),
            Some("1.0.0-beta+exp.sha.5114f85".to_string())
        );
    }

    #[test]
    fn test_apply_ext_mdcode() {
        let text = "Here is code:\n```rust\nfn main() {}\n```\nAnd more:\n```\nlet x = 1;\n```";
        assert_eq!(
            apply("ext.mdcode", &[], text),
            Some("fn main() {}\n\nlet x = 1;".to_string())
        );
        // No code blocks
        assert_eq!(apply("ext.mdcode", &[], "plain text"), Some("".to_string()));
    }

    #[test]
    fn test_apply_ext_mdtable() {
        // Single table
        let text = "Here is a table:\n| A | B |\n|---|---|\n| 1 | 2 |\nText";
        assert_eq!(
            apply("ext.mdtable", &[], text),
            Some("| A | B |\n|---|---|\n| 1 | 2 |".to_string())
        );
        // Multiple tables separated by prose
        let multi = "| X |\n|---|\n| 1 |\n\nProse\n\n| Y |\n|---|\n| 2 |";
        assert_eq!(
            apply("ext.mdtable", &[], multi),
            Some("| X |\n|---|\n| 1 |\n\n| Y |\n|---|\n| 2 |".to_string())
        );
        // Table missing separator must NOT be extracted
        let no_sep = "| A | B |\n| 1 | 2 |";
        assert_eq!(apply("ext.mdtable", &[], no_sep), Some("".to_string()));
    }

    #[test]
    fn test_apply_ext_mdlist() {
        // Unordered list with nested item
        let text = "List:\n* Item 1\n* Item 2\n  * Subitem\n\nDone.";
        assert_eq!(
            apply("ext.mdlist", &[], text),
            Some("* Item 1\n* Item 2\n  * Subitem".to_string())
        );
        // Numbered (ordered) list
        let numbered = "Steps:\n1. First\n2. Second\n\nEnd.";
        assert_eq!(
            apply("ext.mdlist", &[], numbered),
            Some("1. First\n2. Second".to_string())
        );
        // Mixed bullet markers
        let mixed = "- Alpha\n+ Beta\n\nText";
        assert_eq!(
            apply("ext.mdlist", &[], mixed),
            Some("- Alpha\n+ Beta".to_string())
        );
    }
}
