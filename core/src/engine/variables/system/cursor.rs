/// Logic for handling the `{cursor}` system variable.
///
/// This variable defines the final OS-level caret position after snippet expansion.
/// It is processed after all other variables are interpolated.
pub(crate) fn process(text: &mut String, trigger: Option<&str>) -> usize {
    let mut left_arrow_count = 0;

    // Warn if multiple cursor tags are found as only one can define the final position.
    if text.matches("{cursor}").count() > 1 {
        let trigger_ctx = trigger
            .map(|t| format!(" for trigger '{}'", t))
            .unwrap_or_default();
        tracing::warn!(
            "Multiple {{cursor}} tags found in output{}. Only the first occurrence will define the final caret position.",
            trigger_ctx
        );
    }

    // Identify the first occurrence of {cursor} and calculate its offset from the end.
    if let Some(cursor_idx) = text.find("{cursor}") {
        let char_idx = text[..cursor_idx].chars().count();
        *text = text.replace("{cursor}", "");
        left_arrow_count = text.chars().count() - char_idx;
    }

    // Restore escaped \{cursor\} tags.
    *text = text.replace(r#"\{cursor\}"#, "{cursor}");

    left_arrow_count
}
