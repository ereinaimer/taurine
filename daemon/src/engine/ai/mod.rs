use tokio::task::JoinHandle;

pub mod spinner;
pub mod stream;

#[allow(dead_code)]
pub struct InlineAiSpinnerHandle {
    pub cancel: tokio::sync::oneshot::Sender<()>,
    pub task: JoinHandle<()>,
}
