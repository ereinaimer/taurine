use crate::engine::variables::ExpansionStep;
use crate::engine::variables::system::clip::MAX_PAYLOAD_BYTES;

impl crate::engine::evaluator::Evaluator {
    pub(crate) fn allows_blind_undo(&self, steps: &[ExpansionStep]) -> bool {
        let mut text_bytes = 0usize;

        for step in steps {
            match step {
                ExpansionStep::Text(text) => {
                    text_bytes = text_bytes.saturating_add(text.len());
                }
                // Structural templates move the caret away from the absolute tail, so a blind
                // backspace replay would corrupt surrounding text instead of the expansion.
                ExpansionStep::KeyPress(_)
                | ExpansionStep::Delay(_)
                | ExpansionStep::MouseClick(_)
                | ExpansionStep::MouseDblClick(_)
                | ExpansionStep::MouseDown(_)
                | ExpansionStep::MouseUp(_)
                | ExpansionStep::MouseMove(_, _)
                | ExpansionStep::MouseScroll(_) => return false,
                ExpansionStep::Script(_)
                | ExpansionStep::InlineRun(_, _)
                | ExpansionStep::Image(_, _) => return false,
            }
        }

        // Clipboard history can legally hold a full 1 MB payload. Treat that ceiling as unsafe
        // for blind undo so Taurine never floods the OS with a huge backspace replay.
        text_bytes < MAX_PAYLOAD_BYTES
    }

    pub(crate) fn undo_trigger_for_steps(
        &self,
        keyword: &str,
        steps: &[ExpansionStep],
    ) -> Option<String> {
        self.allows_blind_undo(steps).then(|| keyword.to_string())
    }
}
