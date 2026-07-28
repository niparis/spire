#![forbid(unsafe_code)]

//! Adapter composition boundary.
//!
//! Live Linear, GitHub, SQLite, filesystem, systemd, and harness adapters are
//! intentionally deferred to later sprints. Keeping this crate present now makes
//! the permitted dependency direction explicit.

pub struct AdapterBoundary;
