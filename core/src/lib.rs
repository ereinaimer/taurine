//! # Taurine Core
//!
//! The architectural foundation of the Taurine text expander.
//!
//! This crate provides the foundational abstractions and business logic used by
//! the daemon and CLI:
//! - **Engine**: The core interpolation and state management logic.
//! - **DB**: SQLite-backed persistent storage for automations and settings.
//! - **RPC**: Cross-process communication between CLI and Daemon.
//! - **Settings**: Typed configuration management.
//! - **Metrics**: Tracking usage statistics
//! - **Error**: Centralized error handling.

pub mod ai;
pub mod constants;
pub mod db;
pub mod engine;
pub mod error;
pub mod exchange;
pub mod metrics;

pub use error::{Error, Result};

pub mod logs;
pub mod paths;
pub mod rpc;
pub mod settings;
pub mod utils;

#[cfg(test)]
pub use utils::test_utils as testing;
