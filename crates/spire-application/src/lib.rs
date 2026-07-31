#![forbid(unsafe_code)]

//! Use-case contracts and configuration validation for Spire.

pub mod authentication;
pub mod claim;
pub mod config;
pub mod config_migration;
pub mod execution;
pub mod github;
pub mod ingestion;
pub mod installation;
pub mod linear;
pub mod onboarding;
pub mod operations;
pub mod outbox;
pub mod ports;
pub mod project_mapping;
pub mod projection;
pub mod review;
pub mod rollout;
pub mod scheduler;
pub mod webhook;

pub use authentication::*;
pub use claim::*;
pub use config::*;
pub use config_migration::*;
pub use execution::*;
pub use github::*;
pub use ingestion::*;
pub use installation::*;
pub use linear::*;
pub use onboarding::*;
pub use operations::*;
pub use outbox::*;
pub use ports::*;
pub use project_mapping::*;
pub use projection::*;
pub use review::*;
pub use rollout::*;
pub use scheduler::*;
pub use webhook::*;
