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
/// cursor and a one-line description of what each option does.
/// Options are plain labels; the caller maps the confirmed label
/// back onto its domain value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryHeaderMenuState {
    kind: HeaderMenuKind,
    options: Vec<String>,
    details: Vec<String>,
    selected: usize,
}

impl LibraryHeaderMenuState {
    pub(crate) fn new(kind: HeaderMenuKind, current: &str) -> Self {
        // honey: labels derive from the core sets so a new variant
        // lands in the menu automatically; details describe each one.
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
        let details: Vec<String> = options
            .iter()
            .map(|option| match kind {
                HeaderMenuKind::InvocationType => match option.as_str() {
                    "hotkey" => "Fires on a keyboard shortcut",
                    "regex" => "Fires when typed text matches a pattern",
                    "voice" => "Fires on a spoken phrase",
                    _ => "Expands when you type the text",
                },
                HeaderMenuKind::Interpreter => match option.as_str() {
                    "powershell" => "Windows PowerShell scripts",
                    "python" => "Python scripts",
                    "node" => "Node.js scripts",
                    "cmd" => "Windows command scripts",
                    _ => "Unix shell scripts",
                },
                HeaderMenuKind::Behavior => match option.as_str() {
                    "silent" => "Runs in the background, types nothing",
                    _ => "Script output is typed out",
                },
            })
            .map(str::to_string)
            .collect();
        let selected = options
            .iter()
            .position(|option| option == current)
            .unwrap_or(0);
        Self {
            kind,
            options,
            details,
            selected,
        }
    }

    pub(crate) const fn kind(&self) -> HeaderMenuKind {
        self.kind
    }

    pub(crate) fn options(&self) -> &[String] {
        &self.options
    }

    pub(crate) fn details(&self) -> &[String] {
        &self.details
    }

    pub(crate) const fn selected(&self) -> usize {
        self.selected
    }

    pub(crate) fn set_selected(&mut self, index: usize) {
        if index < self.options.len() {
            self.selected = index;
        }
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
