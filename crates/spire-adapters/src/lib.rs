#![forbid(unsafe_code)]

//! Adapter composition boundary.
//!
//! Provider and persistence adapters live behind application-owned ports.

pub struct AdapterBoundary;

pub mod cleanup;
pub mod github;
pub mod github_app;
pub mod harness;
pub mod linear;
pub mod secrets;
pub mod sqlite;
pub mod workspace;
