/// A single row from the `metrics` table.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricRow {
    pub date: String,
    pub executions: i64,
    pub keystrokes_saved: i64,
    pub version: i64,
    pub updated_at: i64,
}
