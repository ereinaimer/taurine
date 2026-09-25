/// A single row from the `stats` table.
#[derive(Debug, Clone, PartialEq)]
pub struct StatRow {
    pub date: String,
    pub executions: i64,
    pub ai_executions: i64,
    pub voice_executions: i64,
    pub words_dictated: i64,
    pub keystrokes_saved: i64,
    pub time_saved_ms: i64,
    pub version: i64,
    pub updated_at: i64,
}

/// Changes to apply to a row in the `stats` table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatDeltas {
    pub executions: i64,
    pub ai_executions: i64,
    pub voice_executions: i64,
    pub words_dictated: i64,
    pub keystrokes_saved: i64,
    pub time_saved_ms: i64,
}
