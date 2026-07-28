#![forbid(unsafe_code)]

//! Use-case contracts and configuration validation for Spire.

pub mod config;
pub mod linear;
pub mod ports;

pub use config::*;
pub use linear::*;
pub use ports::*;
