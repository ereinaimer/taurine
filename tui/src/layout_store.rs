//! Hidden persistence for the pane layout: the two main divider ratios and
//! the create-popup split.
//!
//! Stored as one internal settings row (`tui_pane_layout`) so the TUI comes
//! back the way it was left. No CLI flag and no Settings row: the JSON is
//! versioned so a future shape change can be ignored instead of crashing.

use serde_json::Value;

use crate::widgets::home::state::{DEFAULT_ACTIVITY_RATIO, MAX_ACTIVITY_RATIO, MIN_ACTIVITY_RATIO};
use crate::widgets::library::state::{CREATE_DEFAULT_SPLIT, CREATE_MAX_SPLIT, CREATE_MIN_SPLIT};
use crate::widgets::library::{
    DEFAULT_DETAIL_RATIO, DEFAULT_SPLIT_RATIO, MAX_DETAIL_RATIO, MAX_SPLIT_RATIO, MIN_DETAIL_RATIO,
    MIN_SPLIT_RATIO,
};

pub(crate) const TUI_LAYOUT_KEY: &str = "tui_pane_layout";
pub(crate) const TUI_LAYOUT_VERSION: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TuiLayout {
    pub(crate) list_ratio: f32,
    pub(crate) detail_ratio: f32,
    pub(crate) create_split: f32,
    pub(crate) home_ratio: f32,
}

impl Default for TuiLayout {
    fn default() -> Self {
        Self {
            list_ratio: DEFAULT_SPLIT_RATIO,
            detail_ratio: DEFAULT_DETAIL_RATIO,
            create_split: CREATE_DEFAULT_SPLIT,
            home_ratio: DEFAULT_ACTIVITY_RATIO,
        }
    }
}

/// Clamps one ratio into its own range; a non-finite value falls back to
/// that field's default so a corrupt read can never park a pane.
fn clamped(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

/// Clamp in f64 before the cast so a finite JSON value above `f32::MAX`
/// lands on the range ceiling instead of becoming an infinity default.
fn ratio_from_json(value: &Value, min: f32, max: f32) -> Option<f32> {
    let ratio = value.as_f64()?;
    Some(ratio.clamp(f64::from(min), f64::from(max)) as f32)
}

impl TuiLayout {
    pub(crate) fn sanitized(self) -> Self {
        Self {
            list_ratio: clamped(
                self.list_ratio,
                MIN_SPLIT_RATIO,
                MAX_SPLIT_RATIO,
                DEFAULT_SPLIT_RATIO,
            ),
            detail_ratio: clamped(
                self.detail_ratio,
                MIN_DETAIL_RATIO,
                MAX_DETAIL_RATIO,
                DEFAULT_DETAIL_RATIO,
            ),
            create_split: clamped(
                self.create_split,
                CREATE_MIN_SPLIT,
                CREATE_MAX_SPLIT,
                CREATE_DEFAULT_SPLIT,
            ),
            home_ratio: clamped(
                self.home_ratio,
                MIN_ACTIVITY_RATIO,
                MAX_ACTIVITY_RATIO,
                DEFAULT_ACTIVITY_RATIO,
            ),
        }
    }

    pub(crate) fn to_json(self) -> Value {
        let layout = self.sanitized();
        serde_json::json!({
            "version": TUI_LAYOUT_VERSION,
            "list_ratio": layout.list_ratio,
            "detail_ratio": layout.detail_ratio,
            "create_split": layout.create_split,
            "home_ratio": layout.home_ratio,
        })
    }

    pub(crate) fn from_json(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        if object.get("version")?.as_u64()? != TUI_LAYOUT_VERSION {
            return None;
        }
        Some(
            Self {
                list_ratio: ratio_from_json(
                    object.get("list_ratio")?,
                    MIN_SPLIT_RATIO,
                    MAX_SPLIT_RATIO,
                )?,
                detail_ratio: ratio_from_json(
                    object.get("detail_ratio")?,
                    MIN_DETAIL_RATIO,
                    MAX_DETAIL_RATIO,
                )?,
                create_split: ratio_from_json(
                    object.get("create_split")?,
                    CREATE_MIN_SPLIT,
                    CREATE_MAX_SPLIT,
                )?,
                // honey: layouts written before the home dashboard have
                // no home_ratio; they open at the default width instead
                // of discarding the library widths.
                home_ratio: object
                    .get("home_ratio")
                    .and_then(|value| {
                        ratio_from_json(value, MIN_ACTIVITY_RATIO, MAX_ACTIVITY_RATIO)
                    })
                    .unwrap_or(DEFAULT_ACTIVITY_RATIO),
            }
            .sanitized(),
        )
    }

    pub(crate) fn to_raw(self) -> String {
        self.to_json().to_string()
    }

    pub(crate) fn from_raw(raw: &str) -> Option<Self> {
        Self::from_json(&serde_json::from_str(raw).ok()?)
    }
}

pub(crate) fn load_from<F>(get: F) -> Option<TuiLayout>
where
    F: FnOnce(&str) -> Option<String>,
{
    TuiLayout::from_raw(&get(TUI_LAYOUT_KEY)?)
}

pub(crate) fn save_to<F>(put: F, layout: TuiLayout) -> taurine_core::Result<()>
where
    F: FnOnce(&str, &str) -> taurine_core::Result<()>,
{
    put(TUI_LAYOUT_KEY, &layout.to_raw())
}

pub(crate) fn load() -> Option<TuiLayout> {
    let conn = taurine_core::db::init::setup().ok()?;
    load_from(
        |key| match taurine_core::db::crud::get_setting_value(&conn, key) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(%error, key, "failed to read the saved pane layout");
                None
            }
        },
    )
}

pub(crate) fn save(layout: TuiLayout) -> taurine_core::Result<()> {
    let conn = taurine_core::db::init::setup()?;
    save_to(
        |key, raw| {
            taurine_core::db::crud::upsert_setting(&conn, key, raw)
                .map_err(taurine_core::Error::Database)
        },
        layout,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    fn sample() -> TuiLayout {
        TuiLayout {
            list_ratio: 0.3,
            detail_ratio: 0.9,
            create_split: 0.6,
            home_ratio: 0.55,
        }
    }

    #[test]
    fn default_layout_uses_existing_pane_defaults() {
        let layout = TuiLayout::default();
        assert_eq!(layout.list_ratio, DEFAULT_SPLIT_RATIO);
        assert_eq!(layout.detail_ratio, DEFAULT_DETAIL_RATIO);
        assert_eq!(layout.create_split, CREATE_DEFAULT_SPLIT);
        assert_eq!(layout.home_ratio, DEFAULT_ACTIVITY_RATIO);
    }

    #[test]
    fn layout_json_round_trips_all_four_ratios() {
        let layout = sample();
        assert_eq!(TuiLayout::from_json(&layout.to_json()), Some(layout));
        assert_eq!(TuiLayout::from_raw(&layout.to_raw()), Some(layout));
    }

    #[test]
    fn layout_json_has_version_and_exact_field_names() {
        let value = TuiLayout::default().to_json();
        assert_eq!(value["version"], json!(TUI_LAYOUT_VERSION));
        assert!(value["list_ratio"].is_number());
        assert!(value["detail_ratio"].is_number());
        assert!(value["create_split"].is_number());
        assert!(value["home_ratio"].is_number());

        let mut keys: Vec<&str> = value
            .as_object()
            .expect("layout json is an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "create_split",
                "detail_ratio",
                "home_ratio",
                "list_ratio",
                "version"
            ]
        );
    }

    #[test]
    fn layout_json_without_home_ratio_keeps_the_other_widths() {
        let value = json!({
            "version": TUI_LAYOUT_VERSION,
            "list_ratio": 0.3,
            "detail_ratio": 0.9,
            "create_split": 0.6,
        });

        let layout = TuiLayout::from_json(&value).expect("legacy layout");

        assert_eq!(layout.list_ratio, 0.3);
        assert_eq!(layout.detail_ratio, 0.9);
        assert_eq!(layout.create_split, 0.6);
        assert_eq!(layout.home_ratio, DEFAULT_ACTIVITY_RATIO);
    }

    #[test]
    fn layout_json_rejects_missing_version() {
        let value = json!({
            "list_ratio": 0.3,
            "detail_ratio": 0.9,
            "create_split": 0.6,
        });
        assert_eq!(TuiLayout::from_json(&value), None);
    }

    #[test]
    fn layout_json_rejects_unknown_version() {
        let value = json!({
            "version": TUI_LAYOUT_VERSION + 1,
            "list_ratio": 0.3,
            "detail_ratio": 0.9,
            "create_split": 0.6,
        });
        assert_eq!(TuiLayout::from_json(&value), None);
    }

    #[test]
    fn layout_json_rejects_missing_ratio() {
        for missing in ["list_ratio", "detail_ratio", "create_split"] {
            let mut value = TuiLayout::default().to_json();
            value
                .as_object_mut()
                .expect("layout json is an object")
                .remove(missing);
            assert_eq!(TuiLayout::from_json(&value), None, "missing {missing}");
        }
    }

    #[test]
    fn layout_json_rejects_malformed_raw_json() {
        assert_eq!(TuiLayout::from_raw("{ not json"), None);
        assert_eq!(TuiLayout::from_raw(""), None);
        assert_eq!(TuiLayout::from_raw("null"), None);
        assert_eq!(
            TuiLayout::from_raw(r#"{"version":1,"list_ratio":"wide"}"#),
            None
        );
    }

    #[test]
    fn layout_json_clamps_out_of_range_ratios() {
        let value = json!({
            "version": TUI_LAYOUT_VERSION,
            "list_ratio": -3.0,
            "detail_ratio": 99.0,
            "create_split": 5.0,
            "home_ratio": -1.0,
        });
        let layout = TuiLayout::from_json(&value).expect("valid layout");
        assert_eq!(layout.list_ratio, MIN_SPLIT_RATIO);
        assert_eq!(layout.detail_ratio, MAX_DETAIL_RATIO);
        assert_eq!(layout.create_split, CREATE_MAX_SPLIT);
        assert_eq!(layout.home_ratio, MIN_ACTIVITY_RATIO);

        let clamped = TuiLayout {
            list_ratio: 9.0,
            detail_ratio: -1.0,
            create_split: 0.0,
            home_ratio: 9.0,
        }
        .to_json();
        assert_eq!(clamped["list_ratio"], json!(MAX_SPLIT_RATIO));
        assert_eq!(clamped["detail_ratio"], json!(MIN_DETAIL_RATIO));
        assert_eq!(clamped["create_split"], json!(CREATE_MIN_SPLIT));
        assert_eq!(clamped["home_ratio"], json!(MAX_ACTIVITY_RATIO));
    }

    #[test]
    fn layout_json_clamps_f64_values_above_f32_range() {
        let value = json!({
            "version": TUI_LAYOUT_VERSION,
            "list_ratio": 1e39,
            "detail_ratio": 1e39,
            "create_split": 1e39,
            "home_ratio": 1e39,
        });

        let layout = TuiLayout::from_json(&value).expect("valid layout");

        assert_eq!(layout.list_ratio, MAX_SPLIT_RATIO);
        assert_eq!(layout.detail_ratio, MAX_DETAIL_RATIO);
        assert_eq!(layout.create_split, CREATE_MAX_SPLIT);
        assert_eq!(layout.home_ratio, MAX_ACTIVITY_RATIO);
    }

    #[test]
    fn layout_sanitized_replaces_non_finite_values() {
        let layout = TuiLayout {
            list_ratio: f32::NAN,
            detail_ratio: f32::INFINITY,
            create_split: f32::NEG_INFINITY,
            home_ratio: f32::NAN,
        }
        .sanitized();
        assert_eq!(layout, TuiLayout::default());
    }

    #[test]
    fn load_from_reads_the_internal_key() {
        let layout = sample();
        let raw = layout.to_raw();
        let seen = RefCell::new(None);
        let loaded = load_from(|key| {
            *seen.borrow_mut() = Some(key.to_string());
            Some(raw.clone())
        });
        assert_eq!(loaded, Some(layout));
        assert_eq!(seen.into_inner(), Some(TUI_LAYOUT_KEY.to_string()));
    }

    #[test]
    fn load_from_returns_none_for_missing_key() {
        assert_eq!(load_from(|_| None), None);
    }

    #[test]
    fn save_to_writes_the_internal_key_and_decodable_payload() {
        let layout = sample();
        let seen = RefCell::new(None);
        save_to(
            |key, raw| {
                *seen.borrow_mut() = Some((key.to_string(), raw.to_string()));
                Ok(())
            },
            layout,
        )
        .expect("save succeeds");

        let (key, raw) = seen.into_inner().expect("putter was called");
        assert_eq!(key, TUI_LAYOUT_KEY);
        assert_eq!(TuiLayout::from_raw(&raw), Some(layout));
    }

    #[test]
    fn save_to_propagates_write_error() {
        let result = save_to(
            |_, _| Err(taurine_core::Error::Config("disk full".to_string())),
            TuiLayout::default(),
        );
        assert!(matches!(
            result,
            Err(taurine_core::Error::Config(message)) if message == "disk full"
        ));
    }
}
