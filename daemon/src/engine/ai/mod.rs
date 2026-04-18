use std::sync::Mutex;

use tokio::task::JoinHandle;

pub mod spinner;

#[allow(dead_code)]
pub struct InlineAiSpinnerHandle {
    pub cancel: tokio::sync::oneshot::Sender<()>,
    pub task: JoinHandle<()>,
}

#[derive(Default)]
pub struct InlineAiUiState {
    spinner: Mutex<Option<InlineAiSpinnerHandle>>,
}

impl InlineAiUiState {
    pub fn set_spinner(&self, handle: InlineAiSpinnerHandle) {
        if let Ok(mut guard) = self.spinner.lock() {
            *guard = Some(handle);
        }
    }

    #[allow(dead_code)]
    pub fn take_spinner(&self) -> Option<InlineAiSpinnerHandle> {
        self.spinner.lock().ok()?.take()
    }
}
