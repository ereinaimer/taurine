//! # Taurine Core
//!
//! The architectural foundation of the Taurine text expander.
//!
//! This crate provides the foundational abstractions and business logic used by
//! the daemon and CLI:
//! - **Engine**: The core interpolation and state management logic.
//! - **DB**: SQLite-backed persistent storage for triggers and settings.
//! - **RPC**: Cross-process communication between CLI and Daemon.
//! - **Settings**: Typed configuration management.
//! - **Stats**: Tracking usage statistics
//! - **Error**: Centralized error handling.

pub mod ai;
pub mod db;
pub mod engine;
pub mod exchange;
pub mod keys;
pub mod stats;

pub use error::{Error, Result};

pub mod logs;
pub mod service;
pub mod settings;
pub mod system;
pub mod utils;

pub use system::{constants, error, paths, rpc};

#[cfg(test)]
pub use utils::test_utils as testing;
