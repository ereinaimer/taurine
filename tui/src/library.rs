use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use taurine_core::db::crud::{AutomationListItem, TriggerType};

const LIBRARY_FOOTER: &str = "/ Search   n New   e Edit   d Delete   Enter Details   q Quit";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryKind {
    Snippet,
    Script,
    HotkeySnippet,
    HotkeyScript,
}

impl LibraryKind {
    pub(crate) fn from_parts(trigger_type: TriggerType, action_type: &str) -> Self {
        let is_script = action_type.eq_ignore_ascii_case("script");

        match (trigger_type, is_script) {
            (TriggerType::Hotkey, true) => Self::HotkeyScript,
            (TriggerType::Hotkey, false) => Self::HotkeySnippet,
            (TriggerType::Word, true) => Self::Script,
            (TriggerType::Word, false) => Self::Snippet,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Snippet => "snippet",
            Self::Script => "script",
            Self::HotkeySnippet => "hotkey snippet",
            Self::HotkeyScript => "hotkey script",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryAutomation {
    trigger: String,
    preview: String,
    kind: LibraryKind,
    target_os: String,
    raw_target_os: String,
    uses: u64,
}

impl LibraryAutomation {
    pub(crate) fn trigger(&self) -> &str {
        &self.trigger
    }

    pub(crate) fn preview(&self) -> &str {
        &self.preview
    }

    pub(crate) const fn kind_label(&self) -> &'static str {
        self.kind.label()
    }

    pub(crate) fn metadata_label(&self) -> String {
        format!("{} // {} uses", self.target_os, self.uses)
    }

    fn matches_query(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }

        let needle = query.to_ascii_lowercase();
        self.trigger.to_ascii_lowercase().contains(&needle)
            || self.preview.to_ascii_lowercase().contains(&needle)
            || self.kind_label().contains(&needle)
            || self.target_os.to_ascii_lowercase().contains(&needle)
            || self.raw_target_os.to_ascii_lowercase().contains(&needle)
    }
}

impl From<AutomationListItem> for LibraryAutomation {
    fn from(item: AutomationListItem) -> Self {
        let kind = LibraryKind::from_parts(item.trigger_type, item.action_type.as_str());
        let preview = preview_from_output(&item.output, item.action_type.as_str());

        Self {
            trigger: item.trigger,
            preview,
            kind,
            target_os: display_target_os(&item.target_os).to_string(),
            raw_target_os: item.target_os,
            uses: item.usage_count.max(0) as u64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct LibraryPageState {
    items: Vec<LibraryAutomation>,
    filtered_indices: Vec<usize>,
    selected: usize,
    search_query: String,
    search_mode: bool,
    load_error: Option<String>,
}

impl LibraryPageState {
    pub(crate) fn replace_items(&mut self, mut items: Vec<LibraryAutomation>) {
        sort_items(&mut items);
        self.items = items;
        self.load_error = None;
        self.rebuild_filter();
    }

    pub(crate) fn set_load_error(&mut self, error: String) {
        self.load_error = Some(error);
    }

    pub(crate) fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub(crate) fn footer_text(&self) -> &'static str {
        LIBRARY_FOOTER
    }

    pub(crate) fn search_query(&self) -> &str {
        &self.search_query
    }

    pub(crate) const fn is_search_active(&self) -> bool {
        self.search_mode
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        if self.filtered_indices.is_empty() {
            None
        } else {
            Some(
                self.selected
                    .min(self.filtered_indices.len().saturating_sub(1)),
            )
        }
    }

    pub(crate) fn filtered_len(&self) -> usize {
        self.filtered_indices.len()
    }

    pub(crate) fn item_at_filtered(&self, index: usize) -> Option<&LibraryAutomation> {
        self.filtered_indices
            .get(index)
            .and_then(|item_index| self.items.get(*item_index))
    }

    pub(crate) fn empty_state_message(&self) -> Option<&'static str> {
        if self.items.is_empty() {
            Some("No automations yet.")
        } else if self.filtered_indices.is_empty() {
            Some("No automations match your search.")
        } else {
            None
        }
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) {
        if self.search_mode {
            self.handle_search_key(key);
            return;
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('/'), KeyModifiers::NONE) => self.search_mode = true,
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                self.move_selection(1)
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                self.move_selection(-1)
            }
            _ => {}
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) => self.search_mode = false,
            (KeyCode::Enter, KeyModifiers::NONE) => self.search_mode = false,
            (KeyCode::Backspace, KeyModifiers::NONE) => {
                self.search_query.pop();
                self.rebuild_filter();
            }
            (KeyCode::Char(ch), modifiers)
                if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.search_query.push(ch);
                self.rebuild_filter();
            }
            _ => {}
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let Some(current) = self.selected_index() else {
            self.selected = 0;
            return;
        };

        let max_index = self.filtered_indices.len().saturating_sub(1) as isize;
        let next = (current as isize + delta).clamp(0, max_index);
        self.selected = next as usize;
    }

    fn rebuild_filter(&mut self) {
        let previously_selected = self.selected_item().cloned();
        self.filtered_indices = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.matches_query(&self.search_query).then_some(index))
            .collect();

        if self.filtered_indices.is_empty() {
            self.selected = 0;
            return;
        }

        if let Some(previous_item) = previously_selected
            && let Some(position) = self
                .filtered_indices
                .iter()
                .position(|item_index| self.items[*item_index] == previous_item)
        {
            self.selected = position;
            return;
        }

        self.selected = 0;
    }

    fn selected_item(&self) -> Option<&LibraryAutomation> {
        self.selected_index()
            .and_then(|selected| self.item_at_filtered(selected))
    }
}

fn sort_items(items: &mut [LibraryAutomation]) {
    items.sort_by(|left, right| {
        let left_trigger = left.trigger.to_ascii_lowercase();
        let right_trigger = right.trigger.to_ascii_lowercase();

        left_trigger
            .cmp(&right_trigger)
            .then_with(|| left.kind_label().cmp(right.kind_label()))
            .then_with(|| left.target_os.cmp(&right.target_os))
            .then_with(|| left.preview.cmp(&right.preview))
    });
}

fn preview_from_output(output: &str, action_type: &str) -> String {
    let collapsed = output.split_whitespace().collect::<Vec<_>>().join(" ");
    if !collapsed.is_empty() {
        collapsed
    } else if action_type.eq_ignore_ascii_case("script") {
        "Script preview unavailable.".to_string()
    } else {
        "No preview available.".to_string()
    }
}

fn display_target_os(target_os: &str) -> &str {
    match target_os {
        "all" => "all",
        "win" => "windows",
        "mac" => "macos",
        "linux" => "linux",
        "android" => "android",
        "ios" => "ios",
        _ => target_os,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list_item(
        trigger_type: TriggerType,
        trigger: &str,
        output: &str,
        action_type: &str,
        target_os: &str,
        usage_count: i64,
    ) -> AutomationListItem {
        AutomationListItem {
            trigger_type,
            trigger: trigger.to_string(),
            output: output.to_string(),
            action_type: action_type.to_string(),
            target_os: target_os.to_string(),
            usage_count,
            last_used_at: None,
            created_at: 0,
            interpreter: None,
            behavior: None,
        }
    }

    fn sample_state() -> LibraryPageState {
        let mut state = LibraryPageState::default();
        state.replace_items(vec![
            LibraryAutomation::from(list_item(
                TriggerType::Word,
                "gm",
                "Good Morning",
                "text",
                "all",
                9,
            )),
            LibraryAutomation::from(list_item(
                TriggerType::Word,
                "deploy",
                "npm run build && npm publish",
                "script",
                "linux",
                4,
            )),
            LibraryAutomation::from(list_item(
                TriggerType::Hotkey,
                "alt+r",
                "Start-Process https://reddit.com",
                "script",
                "win",
                6,
            )),
        ]);
        state
    }

    #[test]
    fn word_text_maps_to_snippet() {
        let item = LibraryAutomation::from(list_item(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
        ));
        assert_eq!(item.kind_label(), "snippet");
    }

    #[test]
    fn word_script_maps_to_script() {
        let item = LibraryAutomation::from(list_item(
            TriggerType::Word,
            "deploy",
            "npm publish",
            "script",
            "all",
            4,
        ));
        assert_eq!(item.kind_label(), "script");
    }

    #[test]
    fn hotkey_text_maps_to_hotkey_snippet() {
        let item = LibraryAutomation::from(list_item(
            TriggerType::Hotkey,
            "alt+t",
            "Thanks!",
            "text",
            "all",
            12,
        ));
        assert_eq!(item.kind_label(), "hotkey snippet");
    }

    #[test]
    fn hotkey_script_maps_to_hotkey_script() {
        let item = LibraryAutomation::from(list_item(
            TriggerType::Hotkey,
            "alt+r",
            "Start-Process https://reddit.com",
            "script",
            "win",
            6,
        ));
        assert_eq!(item.kind_label(), "hotkey script");
    }

    #[test]
    fn search_matches_trigger() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gm");
    }

    #[test]
    fn search_matches_preview() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "morning".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gm");
    }

    #[test]
    fn search_matches_kind_label() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "hotkey".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "alt+r");
    }

    #[test]
    fn search_matches_target_os() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "windows".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().target_os, "windows");
    }

    #[test]
    fn search_is_case_insensitive() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "GOOD".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::SHIFT));
        }

        assert_eq!(state.filtered_len(), 1);
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gm");
    }

    #[test]
    fn selection_clamps_at_bounds() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(state.selected_index(), Some(0));

        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));

        assert_eq!(state.selected_index(), Some(2));
    }

    #[test]
    fn selection_moves_to_first_match_when_filter_removes_selected_item() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "good".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(state.selected_index(), Some(0));
        assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gm");
    }

    #[test]
    fn empty_list_reports_empty_state() {
        let state = LibraryPageState::default();
        assert_eq!(state.empty_state_message(), Some("No automations yet."));
    }

    #[test]
    fn no_match_search_reports_no_match_state() {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in "zzz".chars() {
            state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }

        assert_eq!(
            state.empty_state_message(),
            Some("No automations match your search.")
        );
    }

    #[test]
    fn metadata_uses_double_slash_separator() {
        let item = LibraryAutomation::from(list_item(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
        ));

        assert_eq!(item.metadata_label(), "all // 9 uses");
    }
}
