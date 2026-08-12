use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppFilterPrefix {
    Exe,
    Class,
    Title,
}

impl AppFilterPrefix {
    pub const ALL: [Self; 3] = [Self::Exe, Self::Class, Self::Title];

    pub const fn as_prefix_str(self) -> &'static str {
        match self {
            Self::Exe => "exe",
            Self::Class => "class",
            Self::Title => "title",
        }
    }

    pub fn parse_prefix(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "exe" => Some(Self::Exe),
            "class" => Some(Self::Class),
            "title" => Some(Self::Title),
            _ => None,
        }
    }

    pub fn valid_prefixes_hint() -> &'static str {
        "exe:, class:, title:"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_filter_prefix_roundtrip() {
        for prefix in AppFilterPrefix::ALL {
            let tag = prefix.as_prefix_str();
            assert_eq!(AppFilterPrefix::parse_prefix(tag), Some(prefix));
        }
    }

    #[test]
    fn test_app_filter_prefix_parse_aliases() {
        assert_eq!(
            AppFilterPrefix::parse_prefix("exe"),
            Some(AppFilterPrefix::Exe)
        );
        assert_eq!(
            AppFilterPrefix::parse_prefix("  CLASS  "),
            Some(AppFilterPrefix::Class)
        );
        assert_eq!(
            AppFilterPrefix::parse_prefix("title"),
            Some(AppFilterPrefix::Title)
        );
        assert_eq!(AppFilterPrefix::parse_prefix("invalid"), None);
    }
}
