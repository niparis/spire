//! Operational policy that remains independent of host, systemd, and SQLite APIs.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ResourceGuardPolicy {
    pub minimum_free_disk_bytes: u64,
    pub minimum_free_inodes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ResourceGuardObservation {
    pub free_disk_bytes: u64,
    pub free_inodes: u64,
    pub workspace_root_healthy: bool,
    pub database_healthy: bool,
    pub runner_available: bool,
    pub repository_in_maintenance: bool,
    pub provider_candidate_healthy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionGuardFailure {
    DiskBelowThreshold,
    InodesBelowThreshold,
    WorkspaceRootUnavailable,
    DatabaseUnhealthy,
    RunnerUnavailable,
    RepositoryMaintenance,
    NoHealthyProvider,
}

/// Failed guards stop new harness admission only. Reconciliation, outbox
/// delivery, recovery, backup, and cleanup retain their independent authority.
pub fn evaluate_admission_guards(
    policy: ResourceGuardPolicy,
    observation: ResourceGuardObservation,
) -> Vec<AdmissionGuardFailure> {
    let mut failures = Vec::new();
    if observation.free_disk_bytes < policy.minimum_free_disk_bytes {
        failures.push(AdmissionGuardFailure::DiskBelowThreshold);
    }
    if observation.free_inodes < policy.minimum_free_inodes {
        failures.push(AdmissionGuardFailure::InodesBelowThreshold);
    }
    if !observation.workspace_root_healthy {
        failures.push(AdmissionGuardFailure::WorkspaceRootUnavailable);
    }
    if !observation.database_healthy {
        failures.push(AdmissionGuardFailure::DatabaseUnhealthy);
    }
    if !observation.runner_available {
        failures.push(AdmissionGuardFailure::RunnerUnavailable);
    }
    if observation.repository_in_maintenance {
        failures.push(AdmissionGuardFailure::RepositoryMaintenance);
    }
    if !observation.provider_candidate_healthy {
        failures.push(AdmissionGuardFailure::NoHealthyProvider);
    }
    failures
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RetentionPolicy {
    pub workspace_terminal_seconds: u64,
    pub evidence_terminal_seconds: u64,
    pub backup_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperationsSnapshot {
    pub inbox_depth: u64,
    pub outbox_depth: u64,
    pub active_runs: u64,
    pub active_ai_runs: u64,
    pub terminal_workspace_cleanup_backlog: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_state_blocks_admission_without_muting_operations() {
        let failures = evaluate_admission_guards(
            ResourceGuardPolicy {
                minimum_free_disk_bytes: 100,
                minimum_free_inodes: 10,
            },
            ResourceGuardObservation {
                free_disk_bytes: 99,
                free_inodes: 10,
                workspace_root_healthy: true,
                database_healthy: true,
                runner_available: true,
                repository_in_maintenance: false,
                provider_candidate_healthy: false,
            },
        );
        assert_eq!(
            failures,
            vec![
                AdmissionGuardFailure::DiskBelowThreshold,
                AdmissionGuardFailure::NoHealthyProvider,
            ]
        );
    }
}
