#![forbid(unsafe_code)]

//! Use-case contracts and configuration validation for Spire.

pub mod config;
pub mod execution;
pub mod linear;
pub mod ports;
pub mod scheduler;

pub use config::*;
pub use execution::*;
pub use linear::*;
pub use ports::*;
pub use scheduler::*;
