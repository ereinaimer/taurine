pub mod add;
pub mod ai;
pub mod auth;
pub mod completions;
pub mod config;
pub mod delete;
pub mod export;
pub mod import;
pub mod list;
pub mod progress;
pub mod script;
pub mod service;
pub mod status;
pub mod update;
pub mod validate;
pub mod workspace;

#[cfg(test)]
mod script_tests;
#[cfg(test)]
pub(crate) mod test_keyring;

#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
