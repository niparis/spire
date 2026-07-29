#![forbid(unsafe_code)]

//! Use-case contracts and configuration validation for Spire.

pub mod claim;
pub mod config;
pub mod execution;
pub mod github;
pub mod ingestion;
pub mod linear;
pub mod operations;
pub mod outbox;
pub mod ports;
pub mod projection;
pub mod review;
pub mod rollout;
pub mod scheduler;
pub mod webhook;

pub use claim::*;
pub use config::*;
pub use execution::*;
pub use github::*;
pub use ingestion::*;
pub use linear::*;
pub use operations::*;
pub use outbox::*;
pub use ports::*;
pub use projection::*;
pub use review::*;
pub use rollout::*;
pub use scheduler::*;
pub use webhook::*;
