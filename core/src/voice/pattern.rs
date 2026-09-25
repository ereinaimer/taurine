/// A segment in a parsed voice invocation pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoicePatternPart {
    /// Static anchor text matched against spoken words.
    Anchor(String),
    /// A named slot placeholder (e.g. `msg` from `[msg]`).
    Slot(String),
}

/// A parsed voice invocation pattern supporting static phrases or carrier phrases with slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicePattern {
    pub parts: Vec<VoicePatternPart>,
}

impl VoicePattern {
    /// Parse a voice pattern string (e.g. `"send [msg] to [person]"` or `"say hi"`).
    pub fn parse(input: &str) -> Result<Self, String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err("Voice pattern cannot be empty".to_string());
        }

        let mut parts = Vec::new();
        let mut rest = trimmed;

        while !rest.is_empty() {
            if let Some(open_idx) = rest.find('[') {
                let leading_anchor = rest[..open_idx].trim();
                if !leading_anchor.is_empty() {
                    parts.push(VoicePatternPart::Anchor(leading_anchor.to_string()));
                }

                let close_idx = rest[open_idx..]
                    .find(']')
                    .map(|i| open_idx + i)
                    .ok_or_else(|| "Unclosed slot bracket '[' in voice pattern".to_string())?;

                let slot_raw = rest[open_idx + 1..close_idx].trim();
                let slot_name = if let Some((name, _)) = slot_raw.split_once('=') {
                    name.trim()
                } else {
                    slot_raw
                };

                if slot_name.is_empty() {
                    return Err("Empty slot name in voice pattern".to_string());
                }

                parts.push(VoicePatternPart::Slot(slot_name.to_string()));
                rest = rest[close_idx + 1..].trim_start();
            } else {
                let trailing_anchor = rest.trim();
                if !trailing_anchor.is_empty() {
                    parts.push(VoicePatternPart::Anchor(trailing_anchor.to_string()));
                }
                break;
            }
        }

        Ok(Self { parts })
    }

    /// Returns true if this pattern contains at least one slot.
    pub fn is_parameterized(&self) -> bool {
        self.parts
            .iter()
            .any(|p| matches!(p, VoicePatternPart::Slot(_)))
    }

    /// Returns the list of slot names declared in this pattern in order.
    pub fn slot_names(&self) -> Vec<String> {
        self.parts
            .iter()
            .filter_map(|p| match p {
                VoicePatternPart::Slot(name) => Some(name.clone()),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_static_phrase() {
        let pattern = VoicePattern::parse("say hi").unwrap();
        assert!(!pattern.is_parameterized());
        assert_eq!(
            pattern.parts,
            vec![VoicePatternPart::Anchor("say hi".to_string())]
        );
        assert!(pattern.slot_names().is_empty());
    }

    #[test]
    fn test_parse_parameterized_phrase() {
        let pattern = VoicePattern::parse("send [msg] to [person]").unwrap();
        assert!(pattern.is_parameterized());
        assert_eq!(
            pattern.parts,
            vec![
                VoicePatternPart::Anchor("send".to_string()),
                VoicePatternPart::Slot("msg".to_string()),
                VoicePatternPart::Anchor("to".to_string()),
                VoicePatternPart::Slot("person".to_string()),
            ]
        );
        assert_eq!(pattern.slot_names(), vec!["msg", "person"]);
    }

    #[test]
    fn test_parse_leading_and_trailing_slots() {
        let pattern = VoicePattern::parse("[greeting] everyone").unwrap();
        assert_eq!(
            pattern.parts,
            vec![
                VoicePatternPart::Slot("greeting".to_string()),
                VoicePatternPart::Anchor("everyone".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_unclosed_or_empty_brackets_fails() {
        assert!(VoicePattern::parse("send [msg to").is_err());
        assert!(VoicePattern::parse("send [] to").is_err());
    }
}
