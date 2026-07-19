/// Logic for calculating stats based on expansion events.
pub struct ExpansionStats {
    pub keystrokes_saved: i64,
    pub time_saved_ms: i64,
}

/// Calculates net keystrokes saved using character counts.
pub fn calculate_saved_keystrokes(output_char_count: usize, trigger_char_count: usize) -> i64 {
    output_char_count.saturating_sub(trigger_char_count) as i64
}

/// Calculates time saved using a five-characters-per-word model.
pub fn calculate_time_saved_ms(keystrokes_saved: i64, wpm: u32) -> i64 {
    if keystrokes_saved <= 0 {
        return 0;
    }

    let chars_per_minute =
        u64::from(crate::settings::Settings::sanitize_wpm(wpm)).saturating_mul(5);
    if chars_per_minute == 0 {
        return 0;
    }

    let saved = keystrokes_saved as u64;
    let time_saved_ms = ((saved.saturating_mul(60_000)) / chars_per_minute) as i64;
    time_saved_ms.min(300_000)
}

pub fn calculate_expansion_stats(
    output_char_count: usize,
    trigger_char_count: usize,
    wpm: u32,
) -> ExpansionStats {
    let keystrokes_saved = calculate_saved_keystrokes(output_char_count, trigger_char_count);
    let time_saved_ms = calculate_time_saved_ms(keystrokes_saved, wpm);

    ExpansionStats {
        keystrokes_saved,
        time_saved_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_stat_subtracts_trigger_chars() {
        assert_eq!(calculate_saved_keystrokes(100, 5), 95);
    }

    #[test]
    fn saved_keystrokes_saturate_at_zero() {
        assert_eq!(calculate_saved_keystrokes(3, 10), 0);
    }

    #[test]
    fn triggerless_expansion_counts_full_output() {
        assert_eq!(calculate_saved_keystrokes(50, 0), 50);
    }

    #[test]
    fn time_saved_uses_wpm() {
        let fast = calculate_time_saved_ms(120, 60);
        let slow = calculate_time_saved_ms(120, 30);
        assert!(slow > fast);
    }

    #[test]
    fn time_saved_caps_at_five_minutes() {
        // At 60 WPM, 100000 keystrokes would take massive time, but should cap at 5 mins (300,000 ms)
        assert_eq!(calculate_time_saved_ms(100_000, 60), 300_000);
    }

    #[test]
    fn zero_or_negative_savings_have_zero_time_saved() {
        assert_eq!(calculate_time_saved_ms(0, 60), 0);
        assert_eq!(calculate_time_saved_ms(-5, 60), 0);
    }

    #[test]
    fn zero_wpm_falls_back_to_default() {
        assert_eq!(
            calculate_time_saved_ms(60, 0),
            calculate_time_saved_ms(60, crate::settings::Settings::default_wpm())
        );
    }
}
