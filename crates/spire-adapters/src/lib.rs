#![forbid(unsafe_code)]

//! Adapter composition boundary.
//!
//! Provider and persistence adapters live behind application-owned ports.

pub struct AdapterBoundary;

pub mod github;
pub mod harness;
pub mod linear;
pub mod sqlite;
pub mod workspace;
