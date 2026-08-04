use std::sync::{Arc, OnceLock};

use arc_swap::ArcSwap;

use crate::db::crud::TriggerAction;
use crate::engine::catalog::{entry_has_app_filters, is_app_allowed};
use crate::keys::{Hotkey, LogicalKey, hotkey_matches, parse_hotkey};
struct WindowResolver {
    cached: OnceLock<Option<String>>,
}

impl WindowResolver {
    pub fn lazy() -> Self {
        Self {
            cached: OnceLock::new(),
        }
    }

    pub fn resolve(&self, fetcher: impl FnOnce() -> Option<String>) -> Option<&str> {
        self.cached.get_or_init(fetcher).as_deref()
    }

    #[allow(dead_code)]
    pub fn get_cached(&self) -> Option<&str> {
        self.cached.get().and_then(|o| o.as_deref())
    }
}
pub struct HotkeyCatalog {
    snapshot: ArcSwap<CatalogSnapshot>,
}

#[derive(Default)]
struct CatalogSnapshot {
    parsed_actions: std::collections::HashMap<LogicalKey, Vec<ParsedHotkeyAction>>,
}

#[derive(Clone)]
struct ParsedHotkeyAction {
    configured_trigger: String,
    hotkey: Hotkey,
    action: Arc<TriggerAction>,
}

impl Default for HotkeyCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyCatalog {
    pub fn new() -> Self {
        Self {
            snapshot: ArcSwap::new(Arc::new(CatalogSnapshot::default())),
        }
    }

    pub fn load_actions(&self, actions: impl IntoIterator<Item = (String, TriggerAction)>) {
        let mut snapshot = CatalogSnapshot::default();

        for (trigger, action) in actions {
            if let Ok(hotkey) = parse_hotkey(&trigger) {
                let entry = ParsedHotkeyAction {
                    configured_trigger: trigger.clone(),
                    hotkey,
                    action: Arc::new(action),
                };

                // Only bucket into parsed_actions so that multiple triggers
                // sharing the same hotkey but different app filters all survive.
                // The old canonical_string fast-path HashMap silently overwrote
                // earlier entries whenever two triggers parsed to the same hotkey.
                snapshot
                    .parsed_actions
                    .entry(hotkey.logical_key())
                    .or_default()
                    .push(entry);
            }
        }

        self.snapshot.store(Arc::new(snapshot));
    }

    pub fn has_entry_for(&self, key: LogicalKey) -> bool {
        self.snapshot.load().parsed_actions.contains_key(&key)
    }

    pub fn get_action(&self, trigger: &str) -> Option<TriggerAction> {
        let hotkey = parse_hotkey(trigger).ok()?;
        let base_key = hotkey.logical_key();
        let guard = self.snapshot.load();
        guard.parsed_actions.get(&base_key).and_then(|bucket| {
            bucket
                .iter()
                .find(|entry| entry.configured_trigger == trigger || entry.hotkey == hotkey)
                .map(|entry| entry.action.as_ref().clone())
        })
    }

    pub fn match_action(
        &self,
        pressed: Hotkey,
        active_window: Option<&str>,
    ) -> Option<(String, TriggerAction)> {
        let base_key = pressed.logical_key();
        let guard = self.snapshot.load();
        let bucket = guard.parsed_actions.get(&base_key)?;
        let pressed_canonical = pressed.canonical_string();

        // First pass: prefer an entry whose hotkey canonically matches the
        // pressed combo exactly (e.g. `ralt+m` wins over `alt+m` when the
        // right Alt key is pressed).
        if let Some(entry) = bucket.iter().find(|e| {
            e.hotkey.canonical_string() == pressed_canonical
                && is_app_allowed(&e.action, active_window)
        }) {
            return Some((
                entry.configured_trigger.clone(),
                entry.action.as_ref().clone(),
            ));
        }

        // Second pass: accept any entry whose hotkey overlaps the pressed combo
        // (handles generic modifiers like `alt+m` matching `lalt+m` presses).
        bucket
            .iter()
            .find(|e| hotkey_matches(e.hotkey, pressed) && is_app_allowed(&e.action, active_window))
            .map(|e| (e.configured_trigger.clone(), e.action.as_ref().clone()))
    }

    pub fn match_action_lazy(
        &self,
        pressed: Hotkey,
        fetch_window: impl FnOnce() -> Option<String>,
    ) -> Option<(String, TriggerAction)> {
        let base_key = pressed.logical_key();
        let guard = self.snapshot.load();
        let bucket = guard.parsed_actions.get(&base_key)?;
        let pressed_canonical = pressed.canonical_string();
        let window = WindowResolver::lazy();
        let mut fetch_window = Some(fetch_window);

        // Pass 1: exact canonical match — resolved iteratively
        for entry in bucket.iter() {
            if entry.hotkey.canonical_string() != pressed_canonical {
                continue;
            }
            if !entry_has_app_filters(&entry.action) {
                return Some((
                    entry.configured_trigger.clone(),
                    entry.action.as_ref().clone(),
                ));
            }
            let Some(w) = window.resolve(|| fetch_window.take().unwrap()()) else {
                continue;
            };
            if is_app_allowed(&entry.action, Some(w)) {
                return Some((
                    entry.configured_trigger.clone(),
                    entry.action.as_ref().clone(),
                ));
            }
        }

        // Pass 2: hotkey_matches fallback — resolved iteratively
        for entry in bucket.iter() {
            if !hotkey_matches(entry.hotkey, pressed) {
                continue;
            }
            if !entry_has_app_filters(&entry.action) {
                return Some((
                    entry.configured_trigger.clone(),
                    entry.action.as_ref().clone(),
                ));
            }
            let Some(w) = window.resolve(|| fetch_window.take().unwrap()()) else {
                continue;
            };
            if is_app_allowed(&entry.action, Some(w)) {
                return Some((
                    entry.configured_trigger.clone(),
                    entry.action.as_ref().clone(),
                ));
            }
        }

        None
    }
}
