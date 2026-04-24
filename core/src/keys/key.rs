use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
    Meta,
}

impl Modifier {
    pub const fn canonical_name(self) -> &'static str {
        match self {
            Self::Ctrl => "ctrl",
            Self::Shift => "shift",
            Self::Alt => "alt",
            Self::Meta => "meta",
        }
    }

    pub const fn bit(self) -> u8 {
        match self {
            Self::Ctrl => 1 << 0,
            Self::Shift => 1 << 1,
            Self::Alt => 1 << 2,
            Self::Meta => 1 << 3,
        }
    }

    pub fn from_alias(alias: &str) -> Option<Self> {
        match alias {
            "ctrl" | "control" => Some(Self::Ctrl),
            "shift" => Some(Self::Shift),
            "alt" | "opt" | "option" => Some(Self::Alt),
            "meta" | "cmd" | "command" | "win" | "super" | "mod" => Some(Self::Meta),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(pub(crate) u8);

impl Modifiers {
    pub const NONE: Self = Self(0);

    pub const fn new() -> Self {
        Self::NONE
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn contains(self, modifier: Modifier) -> bool {
        self.0 & modifier.bit() != 0
    }

    pub fn insert(&mut self, modifier: Modifier) -> bool {
        let existed = self.contains(modifier);
        self.0 |= modifier.bit();
        !existed
    }

    pub fn ordered(self) -> impl Iterator<Item = Modifier> {
        [
            Modifier::Ctrl,
            Modifier::Shift,
            Modifier::Alt,
            Modifier::Meta,
        ]
        .into_iter()
        .filter(move |modifier| self.contains(*modifier))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogicalKey {
    Letter(char),
    Digit(u8),
    NumpadDigit(u8),
    Enter,
    Escape,
    Tab,
    Space,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Function(u8),
    Backquote,
    Minus,
    Equal,
    Backslash,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    LeftBracket,
    RightBracket,
    CapsLock,
    NumLock,
    ScrollLock,
    PrintScreen,
    Pause,
    Modifier(Modifier),
}

impl LogicalKey {
    pub fn from_alias(alias: &str) -> Option<Self> {
        if alias.len() == 1 {
            let ch = alias.as_bytes()[0] as char;
            if ch.is_ascii_lowercase() {
                return Some(Self::Letter(ch));
            }
            if ch.is_ascii_digit() {
                return Some(Self::Digit(ch as u8 - b'0'));
            }
        }

        if let Some(rest) = alias.strip_prefix("num")
            && rest.len() == 1
            && rest.as_bytes()[0].is_ascii_digit()
        {
            return Some(Self::NumpadDigit(rest.as_bytes()[0] - b'0'));
        }

        if let Some(rest) = alias.strip_prefix("numpad")
            && rest.len() == 1
            && rest.as_bytes()[0].is_ascii_digit()
        {
            return Some(Self::NumpadDigit(rest.as_bytes()[0] - b'0'));
        }

        if let Some(rest) = alias.strip_prefix('f')
            && let Ok(number) = rest.parse::<u8>()
            && (1..=12).contains(&number)
        {
            return Some(Self::Function(number));
        }

        match alias {
            "enter" | "return" => Some(Self::Enter),
            "esc" | "escape" => Some(Self::Escape),
            "tab" => Some(Self::Tab),
            "space" => Some(Self::Space),
            "backspace" => Some(Self::Backspace),
            "delete" | "del" => Some(Self::Delete),
            "up" | "arrowup" => Some(Self::Up),
            "down" | "arrowdown" => Some(Self::Down),
            "left" | "arrowleft" => Some(Self::Left),
            "right" | "arrowright" => Some(Self::Right),
            "home" => Some(Self::Home),
            "end" => Some(Self::End),
            "pgup" | "pageup" => Some(Self::PageUp),
            "pgdown" | "pagedown" => Some(Self::PageDown),
            "insert" | "ins" => Some(Self::Insert),
            "`" | "backtick" | "grave" | "~" | "tilde" => Some(Self::Backquote),
            "-" | "minus" | "dash" => Some(Self::Minus),
            "=" | "equal" | "equals" => Some(Self::Equal),
            "\\" | "backslash" => Some(Self::Backslash),
            ";" | "semicolon" => Some(Self::Semicolon),
            "'" | "quote" | "apostrophe" => Some(Self::Quote),
            "," | "comma" => Some(Self::Comma),
            "." | "dot" | "period" => Some(Self::Period),
            "/" | "slash" => Some(Self::Slash),
            "[" | "lbracket" | "leftbracket" => Some(Self::LeftBracket),
            "]" | "rbracket" | "rightbracket" => Some(Self::RightBracket),
            "capslock" => Some(Self::CapsLock),
            "numlock" => Some(Self::NumLock),
            "scrolllock" => Some(Self::ScrollLock),
            "printscreen" | "prtsc" => Some(Self::PrintScreen),
            "pause" | "break" => Some(Self::Pause),
            "ctrl" | "control" => Some(Self::Modifier(Modifier::Ctrl)),
            "shift" => Some(Self::Modifier(Modifier::Shift)),
            "alt" | "opt" | "option" => Some(Self::Modifier(Modifier::Alt)),
            "meta" | "cmd" | "command" | "win" | "super" | "mod" => {
                Some(Self::Modifier(Modifier::Meta))
            }
            _ => None,
        }
    }

    pub const fn is_modifier_key(self) -> Option<Modifier> {
        match self {
            Self::Modifier(modifier) => Some(modifier),
            _ => None,
        }
    }

    pub fn canonical_name(self) -> Cow<'static, str> {
        match self {
            Self::Letter(ch) => Cow::Owned(ch.to_string()),
            Self::Digit(digit) => Cow::Owned(digit.to_string()),
            Self::NumpadDigit(digit) => Cow::Owned(format!("num{digit}")),
            Self::Enter => Cow::Borrowed("enter"),
            Self::Escape => Cow::Borrowed("esc"),
            Self::Tab => Cow::Borrowed("tab"),
            Self::Space => Cow::Borrowed("space"),
            Self::Backspace => Cow::Borrowed("backspace"),
            Self::Delete => Cow::Borrowed("delete"),
            Self::Up => Cow::Borrowed("up"),
            Self::Down => Cow::Borrowed("down"),
            Self::Left => Cow::Borrowed("left"),
            Self::Right => Cow::Borrowed("right"),
            Self::Home => Cow::Borrowed("home"),
            Self::End => Cow::Borrowed("end"),
            Self::PageUp => Cow::Borrowed("pgup"),
            Self::PageDown => Cow::Borrowed("pgdown"),
            Self::Insert => Cow::Borrowed("insert"),
            Self::Function(number) => Cow::Owned(format!("f{number}")),
            Self::Backquote => Cow::Borrowed("`"),
            Self::Minus => Cow::Borrowed("-"),
            Self::Equal => Cow::Borrowed("="),
            Self::Backslash => Cow::Borrowed("\\"),
            Self::Semicolon => Cow::Borrowed(";"),
            Self::Quote => Cow::Borrowed("'"),
            Self::Comma => Cow::Borrowed(","),
            Self::Period => Cow::Borrowed("."),
            Self::Slash => Cow::Borrowed("/"),
            Self::LeftBracket => Cow::Borrowed("["),
            Self::RightBracket => Cow::Borrowed("]"),
            Self::CapsLock => Cow::Borrowed("capslock"),
            Self::NumLock => Cow::Borrowed("numlock"),
            Self::ScrollLock => Cow::Borrowed("scrolllock"),
            Self::PrintScreen => Cow::Borrowed("printscreen"),
            Self::Pause => Cow::Borrowed("pause"),
            Self::Modifier(modifier) => Cow::Borrowed(modifier.canonical_name()),
        }
    }
}
