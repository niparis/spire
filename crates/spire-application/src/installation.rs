//! Provider-neutral installation profile and resolved-path DTOs.
//!
//! Environment inspection and filesystem mutation belong to the binary or an
//! adapter.  This module deliberately contains only values that can cross the
//! application boundary without importing either concern.

use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationProfile {
    User,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedPaths {
    pub profile: InstallationProfile,
    pub config_file: PathBuf,
    pub config_root: PathBuf,
    pub data_root: PathBuf,
    pub state_root: PathBuf,
    pub cache_root: PathBuf,
}

impl ResolvedPaths {
    pub fn system() -> Self {
        Self {
            profile: InstallationProfile::System,
            config_file: PathBuf::from("/etc/spire/spire.yaml"),
            config_root: PathBuf::from("/etc/spire"),
            data_root: PathBuf::from("/var/lib/spire"),
            state_root: PathBuf::from("/var/lib/spire/state"),
            cache_root: PathBuf::from("/var/cache/spire"),
        }
    }
}
