use std::time::Duration;

pub const CASE_CYCLE_WINDOW: Duration = Duration::from_millis(5000);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseVariant {
    Original,
    Sentence,
    Title,
    Upper,
    Lower,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CycleDirection {
    Next,
    Prev,
}

pub const CASE_CYCLE_ORDER: [CaseVariant; 5] = [
    CaseVariant::Original,
    CaseVariant::Sentence,
    CaseVariant::Title,
    CaseVariant::Upper,
    CaseVariant::Lower,
];

pub struct CaseCycleSession {
    pub original_text: String,
    pub current_text: String,
    pub variant: CaseVariant,
    pub entered_at: std::time::Instant,
    pub engaged: bool,
}

impl CaseCycleSession {
    pub fn new(original: String) -> Self {
        let current = render_case(&original, CaseVariant::Original);
        Self {
            original_text: original,
            current_text: current,
            variant: CaseVariant::Original,
            entered_at: std::time::Instant::now(),
            engaged: false,
        }
    }

    pub fn is_ready(&self, now: std::time::Instant) -> bool {
        self.engaged || now.saturating_duration_since(self.entered_at) < CASE_CYCLE_WINDOW
    }

    pub fn advance(&mut self, dir: CycleDirection) -> String {
        let start = CASE_CYCLE_ORDER
            .iter()
            .position(|v| *v == self.variant)
            .unwrap();
        let step: isize = match dir {
            CycleDirection::Next => 1,
            CycleDirection::Prev => -1,
        };
        let mut i = start as isize;
        loop {
            i = (i + step).rem_euclid(CASE_CYCLE_ORDER.len() as isize);
            let cand = CASE_CYCLE_ORDER[i as usize];
            let rendered = render_case(&self.original_text, cand);
            if i as usize == start {
                // fully cycled with no change — drop to Original
                self.variant = CaseVariant::Original;
                self.current_text = self.original_text.clone();
                self.engaged = true;
                return self.current_text.clone();
            }
            if rendered != self.current_text {
                self.variant = cand;
                self.current_text = rendered;
                self.engaged = true;
                return self.current_text.clone();
            }
        }
    }
}

pub fn render_case(text: &str, variant: CaseVariant) -> String {
    match variant {
        CaseVariant::Original => text.to_string(),
        CaseVariant::Upper => text.to_uppercase(),
        CaseVariant::Lower => text.to_lowercase(),
        CaseVariant::Sentence => sentence_case(text),
        CaseVariant::Title => title_case(text),
    }
}

fn sentence_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut did_first = false;
    for ch in text.chars() {
        if !did_first && ch.is_alphabetic() {
            out.extend(ch.to_uppercase());
            did_first = true;
        } else {
            out.push(ch);
        }
    }
    out
}

fn title_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut new_word = true;
    for ch in text.chars() {
        if ch.is_whitespace() {
            new_word = true;
            out.push(ch);
        } else if new_word && ch.is_alphabetic() {
            out.extend(ch.to_uppercase());
            new_word = false;
        } else {
            out.push(ch);
            new_word = false;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentence_title_upper_lower_cycle() {
        let base = "hello world";
        assert_eq!(render_case(base, CaseVariant::Sentence), "Hello world");
        assert_eq!(render_case(base, CaseVariant::Title), "Hello World");
        assert_eq!(render_case(base, CaseVariant::Upper), "HELLO WORLD");
        assert_eq!(render_case(base, CaseVariant::Lower), "hello world");
    }

    #[test]
    fn german_szlig_upper_is_ss() {
        assert_eq!(render_case("straße", CaseVariant::Upper), "STRASSE");
    }

    #[test]
    fn no_letters_renders_identity() {
        let base = "2026-08-08";
        assert_eq!(render_case(base, CaseVariant::Sentence), base);
        assert_eq!(render_case(base, CaseVariant::Upper), base);
    }

    #[test]
    fn advance_forward_then_back() {
        let mut s = CaseCycleSession::new("hello world".into());
        assert_eq!(s.variant, CaseVariant::Original);
        // "hello world" original -> "Hello world" Sentence
        assert_eq!(s.advance(CycleDirection::Next), "Hello world");
        // "Hello world" -> "Hello World" Title
        assert_eq!(s.advance(CycleDirection::Next), "Hello World");
        // "Hello World" -> "HELLO WORLD" Upper
        assert_eq!(s.advance(CycleDirection::Next), "HELLO WORLD");
        // "HELLO WORLD" -> "hello world" Lower
        assert_eq!(s.advance(CycleDirection::Next), "hello world");
        // "hello world" -> wraps to "Hello world" Sentence (because Lower and Original are duplicates, and we skip duplicates!)
        assert_eq!(s.advance(CycleDirection::Next), "Hello world");
        // "Hello world" -> "hello world" Lower (going backward)
        assert_eq!(s.advance(CycleDirection::Prev), "hello world");
    }

    #[test]
    fn ready_within_window_only_before_engage() {
        use std::time::Instant;
        let mut s = CaseCycleSession::new("hello".into());
        let now = Instant::now();
        assert!(s.is_ready(now));

        // Exceeded window
        let later = now + Duration::from_secs(6);
        assert!(!s.is_ready(later));

        // Engaged session is always ready
        s.advance(CycleDirection::Next);
        assert!(s.is_ready(later));
    }

    #[test]
    fn identity_variants_still_rotate_without_infinite_loop() {
        let mut s = CaseCycleSession::new("2026".into());
        assert_eq!(s.advance(CycleDirection::Next), "2026");
        assert_eq!(s.variant, CaseVariant::Original);
    }
}
