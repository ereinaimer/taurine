use regex::Regex;
use scraper::{Html, Selector};
use serde_json::Value as JsonValue;
use serde_yml::Value as YamlValue;
use toml::Value as TomlValue;

use super::strip_argument_quotes;

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    match transformer {
        "json" => apply_json(args, content),
        "html" | "xml" => apply_html_xml(args, content),
        "toml" => apply_toml(args, content),
        "yaml" => apply_yaml(args, content),
        "regexmatch" => apply_regexmatch(args, content),
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
}
