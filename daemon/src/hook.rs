#[cfg(not(target_os = "linux"))]
mod case_cycle;
mod completion;
mod dispatch;
mod listener;

#[cfg(windows)]
pub(crate) mod raw_input;
#[cfg(windows)]
mod supervisor;

#[cfg(not(target_os = "linux"))]
#[allow(unused_imports)]
pub(crate) use case_cycle::case_cycle_key_action;
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
