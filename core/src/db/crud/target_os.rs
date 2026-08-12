use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetOs {
    All,
    Windows,
    MacOs,
    Linux,
    Android,
    Ios,
}

impl TargetOs {
    pub const ALL: [Self; 6] = [
        Self::All,
        Self::Windows,
        Self::MacOs,
        Self::Linux,
        Self::Android,
        Self::Ios,
    ];

    pub const fn to_db_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Windows => "win",
            Self::MacOs => "mac",
            Self::Linux => "linux",
            Self::Android => "android",
            Self::Ios => "ios",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Windows => "windows",
            Self::MacOs => "macos",
            Self::Linux => "linux",
            Self::Android => "android",
            Self::Ios => "ios",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "all" => Some(Self::All),
            "win" | "windows" => Some(Self::Windows),
            "mac" | "macos" => Some(Self::MacOs),
            "linux" => Some(Self::Linux),
            "android" => Some(Self::Android),
            "ios" => Some(Self::Ios),
            _ => None,
        }
    }

    pub fn current() -> Self {
        match std::env::consts::OS {
            "windows" => Self::Windows,
            "macos" => Self::MacOs,
            "linux" => Self::Linux,
            "android" => Self::Android,
            "ios" => Self::Ios,
            _ => Self::All,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_os_db_str_and_display_name_roundtrip() {
        for os in TargetOs::ALL {
            let db_str = os.to_db_str();
            let display = os.display_name();
            assert_eq!(TargetOs::parse_str(db_str), Some(os));
            assert_eq!(TargetOs::parse_str(display), Some(os));
        }
    }

    #[test]
    fn test_target_os_parse_aliases() {
        assert_eq!(TargetOs::parse_str("windows"), Some(TargetOs::Windows));
        assert_eq!(TargetOs::parse_str("win"), Some(TargetOs::Windows));
        assert_eq!(TargetOs::parse_str("mac"), Some(TargetOs::MacOs));
        assert_eq!(TargetOs::parse_str("macos"), Some(TargetOs::MacOs));
        assert_eq!(TargetOs::parse_str("bogus"), None);
    }

    #[test]
    fn test_target_os_current_returns_valid_variant() {
        let current = TargetOs::current();
        assert!(TargetOs::ALL.contains(&current));
    }
}
