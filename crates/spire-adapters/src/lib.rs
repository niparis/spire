#![forbid(unsafe_code)]

//! Adapter composition boundary.
//!
//! Linear reads and SQLite are isolated infrastructure boundaries. Linear writes,
//! GitHub, filesystem, systemd, and harness adapters remain deferred.

pub struct AdapterBoundary;

pub mod harness;
pub mod linear;
pub mod sqlite;
pub mod workspace;
