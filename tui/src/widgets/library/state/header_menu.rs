use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

/// Which header button spawned the centered menu: invocation type
/// first, then script language and run behavior for scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeaderMenuKind {
    InvocationType,
    Interpreter,
    Behavior,
}

impl HeaderMenuKind {
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::InvocationType => "Trigger type",
            Self::Interpreter => "Script language",
            Self::Behavior => "Run behavior",
        }
    }
}

/// Centered option menu over the detail pane: option labels with a
/// cursor. Options are plain labels; the caller maps the confirmed
/// label back onto its domain value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryHeaderMenuState {
    kind: HeaderMenuKind,
    options: Vec<String>,
    selected: usize,
}

impl LibraryHeaderMenuState {
    pub(crate) fn new(kind: HeaderMenuKind, current: &str) -> Self {
        let options: Vec<String> = match kind {
            HeaderMenuKind::InvocationType => ["word", "hotkey", "regex", "voice"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            HeaderMenuKind::Interpreter => ScriptInterpreter::ALL
                .iter()
                .map(|interpreter| interpreter.as_str().to_string())
                .collect(),
            HeaderMenuKind::Behavior => ScriptBehavior::ALL
                .iter()
                .map(|behavior| behavior.as_str().to_string())
                .collect(),
        };
        let selected = options
            .iter()
            .position(|option| option == current)
            .unwrap_or(0);
        Self {
            kind,
            options,
            selected,
        }
    }

    pub(crate) const fn kind(&self) -> HeaderMenuKind {
        self.kind
    }

    pub(crate) fn options(&self) -> &[String] {
        &self.options
    }

    pub(crate) const fn selected(&self) -> usize {
        self.selected
    }

    /// Menus wrap around both ends.
    pub(crate) fn move_cursor(&mut self, delta: i32) {
        if self.options.is_empty() {
            return;
        }
        let len = self.options.len() as i32;
        let next = (self.selected as i32 + delta).rem_euclid(len);
        self.selected = next as usize;
    }

    pub(crate) fn selected_option(&self) -> Option<&str> {
        self.options.get(self.selected).map(String::as_str)
    }
}
