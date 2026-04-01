/// A single row from the `settings` table.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingRow {
    /// The unique key that identifies this setting (e.g. `"theme"`, `"fuzzy_finder_prefs"`).
    pub key: String,
    /// The setting's value, stored as a JSON string.
    pub value: String,
    /// Incremented on every write. Used as a Last-Write-Wins arbiter during sync.
    pub version: i64,
    /// Unix timestamp (seconds) of the last write.
    pub updated_at: i64,
}
