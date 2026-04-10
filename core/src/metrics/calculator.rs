/// Logic for calculating metrics based on expansion events.
pub struct ExpansionMetrics {
    pub keystrokes_saved: i64,
}

/// Calculates the net keystrokes saved from an expansion.
///
/// Formula: `output_char_count - delete_count`
///
/// - `output_char_count`: The number of characters in the expansion result.
/// - `delete_count`: The number of backspaces sent to erase the trigger (and suffix).
pub fn calculate_saved_keystrokes(output_char_count: usize, delete_count: usize) -> i64 {
    (output_char_count as i64) - (delete_count as i64)
}
