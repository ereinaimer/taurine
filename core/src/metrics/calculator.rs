/// Logic for calculating metrics based on expansion events.
pub struct ExpansionMetrics {
    pub keystrokes_saved: i64,
}

/// Calculates the net keystrokes saved from an expansion.
///
/// Formula: `(output_char_count + bonus_keystrokes) - delete_count`
///
/// - `output_char_count`: The number of characters in the expansion result.
/// - `delete_count`: The number of backspaces sent to erase the trigger (and suffix).
/// - `bonus_keystrokes`: Extra manual effort saved (e.g., arrow keys for cursor positioning).
pub fn calculate_saved_keystrokes(
    output_char_count: usize,
    delete_count: usize,
    bonus_keystrokes: usize,
) -> i64 {
    let total_gained = (output_char_count + bonus_keystrokes) as i64;
    let total_spent = delete_count as i64;

    // Never record negative savings to avoid confusing the user;
    // at worst, it was an even trade (0 saved).
    (total_gained - total_spent).max(0)
}
