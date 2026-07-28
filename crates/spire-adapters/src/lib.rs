#![forbid(unsafe_code)]

//! Adapter composition boundary.
//!
//! Live Linear, GitHub, filesystem, systemd, and harness adapters are deferred
//! to later sprints. The SQLite adapter is the single-node durability boundary.

pub struct AdapterBoundary;

pub mod sqlite;
