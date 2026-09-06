use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifierFamily {
    Ctrl,
    Shift,
    Alt,
    Meta,
}

impl ModifierFamily {
    pub const fn canonical_name(self) -> &'static str {
        match self {
            Self::Ctrl => "ctrl",
            Self::Shift => "shift",
            Self::Alt => "alt",
            Self::Meta => "meta",
        }
    }

    pub const fn ordered() -> [Self; 4] {
        [Self::Ctrl, Self::Shift, Self::Alt, Self::Meta]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifierSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifierState {
    Absent,
    Generic,
    Left,
    Right,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    Ctrl,
    LeftCtrl,
    RightCtrl,
    Shift,
    LeftShift,
    RightShift,
    Alt,
    LeftAlt,
    RightAlt,
    Meta,
    LeftMeta,
    RightMeta,
}

impl Modifier {
    pub const fn canonical_name(self) -> &'static str {
        match self {
            Self::Ctrl => "ctrl",
            Self::LeftCtrl => "lctrl",
            Self::RightCtrl => "rctrl",
            Self::Shift => "shift",
            Self::LeftShift => "lshift",
            Self::RightShift => "rshift",
            Self::Alt => "alt",
            Self::LeftAlt => "lalt",
            Self::RightAlt => "ralt",
            Self::Meta => "meta",
            Self::LeftMeta => "lmeta",
            Self::RightMeta => "rmeta",
        }
    }

    pub const fn family(self) -> ModifierFamily {
        match self {
            Self::Ctrl | Self::LeftCtrl | Self::RightCtrl => ModifierFamily::Ctrl,
            Self::Shift | Self::LeftShift | Self::RightShift => ModifierFamily::Shift,
            Self::Alt | Self::LeftAlt | Self::RightAlt => ModifierFamily::Alt,
            Self::Meta | Self::LeftMeta | Self::RightMeta => ModifierFamily::Meta,
        }
    }

    pub const fn side(self) -> Option<ModifierSide> {
        match self {
            Self::LeftCtrl | Self::LeftShift | Self::LeftAlt | Self::LeftMeta => {
                Some(ModifierSide::Left)
            }
            Self::RightCtrl | Self::RightShift | Self::RightAlt | Self::RightMeta => {
                Some(ModifierSide::Right)
            }
            Self::Ctrl | Self::Shift | Self::Alt | Self::Meta => None,
        }
    }

    pub const fn generic_for_family(family: ModifierFamily) -> Self {
        match family {
            ModifierFamily::Ctrl => Self::Ctrl,
            ModifierFamily::Shift => Self::Shift,
            ModifierFamily::Alt => Self::Alt,
            ModifierFamily::Meta => Self::Meta,
        }
    }

    pub const fn sided_for_family(family: ModifierFamily, side: ModifierSide) -> Self {
        match (family, side) {
            (ModifierFamily::Ctrl, ModifierSide::Left) => Self::LeftCtrl,
            (ModifierFamily::Ctrl, ModifierSide::Right) => Self::RightCtrl,
            (ModifierFamily::Shift, ModifierSide::Left) => Self::LeftShift,
            (ModifierFamily::Shift, ModifierSide::Right) => Self::RightShift,
            (ModifierFamily::Alt, ModifierSide::Left) => Self::LeftAlt,
            (ModifierFamily::Alt, ModifierSide::Right) => Self::RightAlt,
            (ModifierFamily::Meta, ModifierSide::Left) => Self::LeftMeta,
            (ModifierFamily::Meta, ModifierSide::Right) => Self::RightMeta,
        }
    }

    pub const fn bit(self) -> u16 {
        match self {
            Self::Ctrl => 1 << 0,
            Self::LeftCtrl => 1 << 1,
            Self::RightCtrl => 1 << 2,
            Self::Shift => 1 << 3,
            Self::LeftShift => 1 << 4,
            Self::RightShift => 1 << 5,
            Self::Alt => 1 << 6,
            Self::LeftAlt => 1 << 7,
            Self::RightAlt => 1 << 8,
            Self::Meta => 1 << 9,
            Self::LeftMeta => 1 << 10,
            Self::RightMeta => 1 << 11,
        }
    }

    pub fn from_alias(alias: &str) -> Option<Self> {
        match alias {
            "ctrl" | "control" => Some(Self::Ctrl),
            "lctrl" | "leftctrl" | "leftcontrol" => Some(Self::LeftCtrl),
            "rctrl" | "rightctrl" | "rightcontrol" => Some(Self::RightCtrl),
            "shift" => Some(Self::Shift),
            "lshift" | "leftshift" => Some(Self::LeftShift),
            "rshift" | "rightshift" => Some(Self::RightShift),
            "alt" | "opt" | "option" => Some(Self::Alt),
            "lalt" | "leftalt" | "leftoption" => Some(Self::LeftAlt),
            "ralt" | "rightalt" | "rightoption" | "altgr" => Some(Self::RightAlt),
            "meta" | "cmd" | "command" | "win" | "super" | "mod" => Some(Self::Meta),
            "lmeta" | "leftmeta" | "lwin" | "leftwin" | "leftsuper" | "leftcmd" | "leftcommand" => {
                Some(Self::LeftMeta)
            }
            "rmeta" | "rightmeta" | "rwin" | "rightwin" | "rightsuper" | "rightcmd"
            | "rightcommand" => Some(Self::RightMeta),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifierInsertError {
    Duplicate(Modifier),
    Conflict {
        existing: Modifier,
        incoming: Modifier,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(pub(crate) u16);

impl Modifiers {
    pub const NONE: Self = Self(0);

    pub const fn new() -> Self {
        Self::NONE
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    fn contains_exact(self, modifier: Modifier) -> bool {
        self.0 & modifier.bit() != 0
    }

    pub const fn contains(self, modifier: Modifier) -> bool {
        match modifier.side() {
            Some(_) => self.0 & modifier.bit() != 0,
            None => !matches!(self.family_state(modifier.family()), ModifierState::Absent),
        }
    }

    pub fn insert(&mut self, modifier: Modifier) -> Result<(), ModifierInsertError> {
        let family = modifier.family();
        let existing_state = self.family_state(family);

        if matches!(existing_state, ModifierState::Absent) {
            self.0 |= modifier.bit();
            return Ok(());
        }

        let existing = self
            .canonical_modifier_for_family(family)
            .unwrap_or_else(|| Modifier::generic_for_family(family));

        if self.contains_exact(modifier) {
            return Err(ModifierInsertError::Duplicate(existing));
        }

        Err(ModifierInsertError::Conflict {
            existing,
            incoming: modifier,
        })
    }

    pub fn insert_active(&mut self, modifier: Modifier) {
        self.0 |= modifier.bit();
    }

    pub const fn family_state(self, family: ModifierFamily) -> ModifierState {
        let generic = self.0 & Modifier::generic_for_family(family).bit() != 0;
        let left = self.0 & Modifier::sided_for_family(family, ModifierSide::Left).bit() != 0;
        let right = self.0 & Modifier::sided_for_family(family, ModifierSide::Right).bit() != 0;

        if generic {
            ModifierState::Generic
        } else if left && right {
            ModifierState::Both
        } else if left {
            ModifierState::Left
        } else if right {
            ModifierState::Right
        } else {
            ModifierState::Absent
        }
    }

    pub fn ordered(self) -> impl Iterator<Item = Modifier> {
        ModifierFamily::ordered()
            .into_iter()
            .filter_map(move |family| self.canonical_modifier_for_family(family))
    }

    pub fn overlaps(self, other: Self) -> bool {
        ModifierFamily::ordered().into_iter().all(|family| {
            family_states_overlap(self.family_state(family), other.family_state(family))
        })
    }

    pub fn matches_active(self, active: Self) -> bool {
        ModifierFamily::ordered().into_iter().all(|family| {
            family_requirement_matches(self.family_state(family), active.family_state(family))
        })
    }

    fn canonical_modifier_for_family(self, family: ModifierFamily) -> Option<Modifier> {
        match self.family_state(family) {
            ModifierState::Absent => None,
            ModifierState::Generic => Some(Modifier::generic_for_family(family)),
            ModifierState::Left => Some(Modifier::sided_for_family(family, ModifierSide::Left)),
            ModifierState::Right => Some(Modifier::sided_for_family(family, ModifierSide::Right)),
            ModifierState::Both => Some(Modifier::generic_for_family(family)),
        }
    }
}

const fn family_states_overlap(left: ModifierState, right: ModifierState) -> bool {
    match left {
        ModifierState::Absent => matches!(right, ModifierState::Absent),
        ModifierState::Generic => {
            matches!(
                right,
                ModifierState::Generic
                    | ModifierState::Left
                    | ModifierState::Right
                    | ModifierState::Both
            )
        }
        ModifierState::Left => matches!(right, ModifierState::Generic | ModifierState::Left),
        ModifierState::Right => matches!(right, ModifierState::Generic | ModifierState::Right),
        ModifierState::Both => matches!(right, ModifierState::Generic | ModifierState::Both),
    }
}

const fn family_requirement_matches(required: ModifierState, active: ModifierState) -> bool {
    match required {
        ModifierState::Absent => matches!(active, ModifierState::Absent),
        ModifierState::Generic => {
            matches!(
                active,
                ModifierState::Generic
                    | ModifierState::Left
                    | ModifierState::Right
                    | ModifierState::Both
            )
        }
        ModifierState::Left => matches!(active, ModifierState::Left),
        ModifierState::Right => matches!(active, ModifierState::Right),
        ModifierState::Both => matches!(active, ModifierState::Both),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Button4,
    Button5,
    Other(u8),
}

impl MouseButton {
    pub fn canonical_name(&self) -> Cow<'static, str> {
        match self {
            Self::Left => Cow::Borrowed("mouse1"),
            Self::Right => Cow::Borrowed("mouse2"),
            Self::Middle => Cow::Borrowed("mouse3"),
            Self::Button4 => Cow::Borrowed("mouse4"),
            Self::Button5 => Cow::Borrowed("mouse5"),
            Self::Other(n) => Cow::Owned(format!("mouse{n}")),
        }
    }

    pub fn from_alias(alias: &str) -> Option<Self> {
        let alias = alias.trim().to_ascii_lowercase();
        if let Some(rest) = alias.strip_prefix("mouse")
            && let Ok(number) = rest.parse::<u8>()
        {
            return Some(match number {
                1 => Self::Left,
                2 => Self::Right,
                3 => Self::Middle,
                4 => Self::Button4,
                5 => Self::Button5,
                n => Self::Other(n),
            });
        }

        match alias.as_str() {
            "left" | "1" | "l" | "mouseleft" | "lclick" | "leftclick" | "primary" => {
                Some(Self::Left)
            }
            "right" | "2" | "r" | "mouseright" | "rclick" | "rightclick" | "secondary" => {
                Some(Self::Right)
            }
            "middle" | "3" | "m" | "mousemiddle" | "mclick" | "midclick" | "middleclick"
            | "wheelclick" | "wheel" => Some(Self::Middle),
            "m4" | "4" | "thumb1" | "xbutton1" | "x1" | "back" => Some(Self::Button4),
            "m5" | "5" | "thumb2" | "xbutton2" | "x2" | "forward" => Some(Self::Button5),
            _ => {
                if let Some(rest) = alias.strip_prefix('m')
                    && let Ok(number) = rest.parse::<u8>()
                {
                    Some(match number {
                        1 => Self::Left,
                        2 => Self::Right,
                        3 => Self::Middle,
                        4 => Self::Button4,
                        5 => Self::Button5,
                        n => Self::Other(n),
                    })
                } else {
                    None
                }
            }
        }
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
    Mouse(MouseButton),
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

        if let Some(rest) = alias.strip_prefix("mouse")
            && let Ok(number) = rest.parse::<u8>()
        {
            let button = match number {
                1 => MouseButton::Left,
                2 => MouseButton::Right,
                3 => MouseButton::Middle,
                4 => MouseButton::Button4,
                5 => MouseButton::Button5,
                n => MouseButton::Other(n),
            };
            return Some(Self::Mouse(button));
        }

        match alias {
            "mouseleft" | "lclick" | "leftclick" => Some(Self::Mouse(MouseButton::Left)),
            "mouseright" | "rclick" | "rightclick" => Some(Self::Mouse(MouseButton::Right)),
            "mousemiddle" | "mclick" | "midclick" | "middleclick" | "wheelclick" => {
                Some(Self::Mouse(MouseButton::Middle))
            }
            "m4" | "thumb1" | "xbutton1" | "x1" | "back" => Some(Self::Mouse(MouseButton::Button4)),
            "m5" | "thumb2" | "xbutton2" | "x2" | "forward" => {
                Some(Self::Mouse(MouseButton::Button5))
            }
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
            _ => Modifier::from_alias(alias).map(Self::Modifier),
        }
    }

    pub const fn is_modifier_key(self) -> Option<Modifier> {
        match self {
            Self::Modifier(modifier) => Some(modifier),
            _ => None,
        }
    }

    pub const fn is_mouse_button(self) -> Option<MouseButton> {
        match self {
            Self::Mouse(button) => Some(button),
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
            Self::Mouse(button) => button.canonical_name(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mouse_button_aliases() {
        assert_eq!(
            LogicalKey::from_alias("mouse1"),
            Some(LogicalKey::Mouse(MouseButton::Left))
        );
        assert_eq!(
            LogicalKey::from_alias("mouseleft"),
            Some(LogicalKey::Mouse(MouseButton::Left))
        );
        assert_eq!(
            LogicalKey::from_alias("lclick"),
            Some(LogicalKey::Mouse(MouseButton::Left))
        );
        assert_eq!(
            LogicalKey::from_alias("leftclick"),
            Some(LogicalKey::Mouse(MouseButton::Left))
        );

        assert_eq!(
            LogicalKey::from_alias("mouse2"),
            Some(LogicalKey::Mouse(MouseButton::Right))
        );
        assert_eq!(
            LogicalKey::from_alias("mouseright"),
            Some(LogicalKey::Mouse(MouseButton::Right))
        );
        assert_eq!(
            LogicalKey::from_alias("rclick"),
            Some(LogicalKey::Mouse(MouseButton::Right))
        );
        assert_eq!(
            LogicalKey::from_alias("rightclick"),
            Some(LogicalKey::Mouse(MouseButton::Right))
        );

        assert_eq!(
            LogicalKey::from_alias("mouse3"),
            Some(LogicalKey::Mouse(MouseButton::Middle))
        );
        assert_eq!(
            LogicalKey::from_alias("mousemiddle"),
            Some(LogicalKey::Mouse(MouseButton::Middle))
        );
        assert_eq!(
            LogicalKey::from_alias("mclick"),
            Some(LogicalKey::Mouse(MouseButton::Middle))
        );
        assert_eq!(
            LogicalKey::from_alias("midclick"),
            Some(LogicalKey::Mouse(MouseButton::Middle))
        );
        assert_eq!(
            LogicalKey::from_alias("middleclick"),
            Some(LogicalKey::Mouse(MouseButton::Middle))
        );
        assert_eq!(
            LogicalKey::from_alias("wheelclick"),
            Some(LogicalKey::Mouse(MouseButton::Middle))
        );

        assert_eq!(
            LogicalKey::from_alias("mouse4"),
            Some(LogicalKey::Mouse(MouseButton::Button4))
        );
        assert_eq!(
            LogicalKey::from_alias("m4"),
            Some(LogicalKey::Mouse(MouseButton::Button4))
        );
        assert_eq!(
            LogicalKey::from_alias("thumb1"),
            Some(LogicalKey::Mouse(MouseButton::Button4))
        );
        assert_eq!(
            LogicalKey::from_alias("xbutton1"),
            Some(LogicalKey::Mouse(MouseButton::Button4))
        );
        assert_eq!(
            LogicalKey::from_alias("x1"),
            Some(LogicalKey::Mouse(MouseButton::Button4))
        );
        assert_eq!(
            LogicalKey::from_alias("back"),
            Some(LogicalKey::Mouse(MouseButton::Button4))
        );

        assert_eq!(
            LogicalKey::from_alias("mouse5"),
            Some(LogicalKey::Mouse(MouseButton::Button5))
        );
        assert_eq!(
            LogicalKey::from_alias("m5"),
            Some(LogicalKey::Mouse(MouseButton::Button5))
        );
        assert_eq!(
            LogicalKey::from_alias("thumb2"),
            Some(LogicalKey::Mouse(MouseButton::Button5))
        );
        assert_eq!(
            LogicalKey::from_alias("xbutton2"),
            Some(LogicalKey::Mouse(MouseButton::Button5))
        );
        assert_eq!(
            LogicalKey::from_alias("x2"),
            Some(LogicalKey::Mouse(MouseButton::Button5))
        );
        assert_eq!(
            LogicalKey::from_alias("forward"),
            Some(LogicalKey::Mouse(MouseButton::Button5))
        );

        assert_eq!(
            LogicalKey::from_alias("mouse6"),
            Some(LogicalKey::Mouse(MouseButton::Other(6)))
        );
        assert_eq!(
            LogicalKey::from_alias("mouse7"),
            Some(LogicalKey::Mouse(MouseButton::Other(7)))
        );
    }

    #[test]
    fn mouse_button_canonical_names() {
        assert_eq!(MouseButton::Left.canonical_name(), "mouse1");
        assert_eq!(MouseButton::Right.canonical_name(), "mouse2");
        assert_eq!(MouseButton::Middle.canonical_name(), "mouse3");
        assert_eq!(MouseButton::Button4.canonical_name(), "mouse4");
        assert_eq!(MouseButton::Button5.canonical_name(), "mouse5");
        assert_eq!(MouseButton::Other(6).canonical_name(), "mouse6");

        assert_eq!(
            LogicalKey::Mouse(MouseButton::Left).canonical_name(),
            "mouse1"
        );
        assert_eq!(
            LogicalKey::Mouse(MouseButton::Other(8)).canonical_name(),
            "mouse8"
        );
    }

    #[test]
    fn detects_mouse_buttons() {
        assert_eq!(
            LogicalKey::Mouse(MouseButton::Left).is_mouse_button(),
            Some(MouseButton::Left)
        );
        assert_eq!(LogicalKey::Letter('a').is_mouse_button(), None);
    }
}
