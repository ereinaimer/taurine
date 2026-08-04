mod completion;
mod dispatch;
mod listener;

#[cfg(windows)]
mod supervisor;

#[allow(unused_imports)]
pub(crate) use completion::{
    CompletionKeyAction, CompletionKeyKind, completion_key_action,
    completion_key_kind_from_tab_like, should_swallow_trigger_assist_key_release,
    trigger_assist_is_active, trigger_assist_key_action,
};

#[allow(unused_imports)]
pub(crate) use dispatch::{spawn_completion_rewrite_dispatch, spawn_expansion_dispatch};

#[allow(unused_imports)]
pub use listener::{start_listener, stop_listener};

#[cfg(windows)]
pub use supervisor::{WindowsSupervisorEvent, start_windows_supervisor, stop_windows_supervisor};

#[cfg(test)]
pub(crate) mod tests;
