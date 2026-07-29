//! Durable Linear-project-to-repository routing contracts.
//!
//! The application evaluates only stable project and repository identities.
//! Provider clients, Git commands, and SQLite remain adapter concerns.

use serde::{Deserialize, Serialize};
use spire_domain::{
    LinearProjectId, ProjectMappingRevision, ProjectMappingStatus, ProjectRepositoryMappingId,
    RepositoryName,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRepositoryMapping {
    pub id: ProjectRepositoryMappingId,
    pub linear_organization_id: String,
    pub linear_team_id: String,
    pub linear_project_id: LinearProjectId,
    pub linear_project_name_snapshot: String,
    pub github_repository: RepositoryName,
    pub repository_source_path: String,
    pub git_common_directory: String,
    pub git_remote_url: String,
    pub default_branch: String,
    pub status: ProjectMappingStatus,
    pub revision: ProjectMappingRevision,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProjectRepositoryMapping {
    pub linear_organization_id: String,
    pub linear_team_id: String,
    pub linear_project_id: LinearProjectId,
    pub linear_project_name_snapshot: String,
    pub github_repository: RepositoryName,
    pub repository_source_path: String,
    pub git_common_directory: String,
    pub git_remote_url: String,
    pub default_branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum ProjectRoutingDecision {
    Mapped {
        mapping_id: ProjectRepositoryMappingId,
        mapping_revision: ProjectMappingRevision,
        repository: RepositoryName,
    },
    RepositoryUnmapped,
    MappingDisabled,
    MappingStale,
    MappingAmbiguous,
    RepositoryUnhealthy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingRepositoryHealth {
    Healthy,
    Stale,
    Unhealthy,
}

/// Resolves one project using only durable mapping state. A ticket's title,
/// labels, description, and URLs never participate in this decision.
pub fn resolve_project_routing(
    mappings: &[ProjectRepositoryMapping],
    health: MappingRepositoryHealth,
) -> ProjectRoutingDecision {
    let enabled = mappings
        .iter()
        .filter(|mapping| mapping.status == ProjectMappingStatus::Enabled)
        .collect::<Vec<_>>();
    if enabled.len() > 1 {
        return ProjectRoutingDecision::MappingAmbiguous;
    }
    let Some(mapping) = enabled.first() else {
        return match mappings {
            [] => ProjectRoutingDecision::RepositoryUnmapped,
            [mapping] if mapping.status == ProjectMappingStatus::Disabled => {
                ProjectRoutingDecision::MappingDisabled
            }
            _ => ProjectRoutingDecision::RepositoryUnmapped,
        };
    };
    match health {
        MappingRepositoryHealth::Healthy => ProjectRoutingDecision::Mapped {
            mapping_id: mapping.id,
            mapping_revision: mapping.revision,
            repository: mapping.github_repository.clone(),
        },
        MappingRepositoryHealth::Stale => ProjectRoutingDecision::MappingStale,
        MappingRepositoryHealth::Unhealthy => ProjectRoutingDecision::RepositoryUnhealthy,
    }
}

#[allow(async_fn_in_trait)]
pub trait ProjectMappingPort {
    type Error;

    async fn resolve(
        &self,
        linear_organization_id: &str,
        linear_project_id: &LinearProjectId,
    ) -> Result<ProjectRoutingDecision, Self::Error>;

    async fn list(&self) -> Result<Vec<ProjectRepositoryMapping>, Self::Error>;

    async fn create(
        &self,
        mapping: NewProjectRepositoryMapping,
        actor: &str,
        reason: Option<&str>,
    ) -> Result<ProjectRepositoryMapping, Self::Error>;

    async fn revise(
        &self,
        mapping: ProjectRepositoryMapping,
        expected_revision: ProjectMappingRevision,
        actor: &str,
        reason: &str,
    ) -> Result<ProjectRepositoryMapping, Self::Error>;

    async fn disable(
        &self,
        id: ProjectRepositoryMappingId,
        expected_revision: ProjectMappingRevision,
        actor: &str,
        reason: &str,
    ) -> Result<ProjectRepositoryMapping, Self::Error>;

    async fn tombstone(
        &self,
        id: ProjectRepositoryMappingId,
        expected_revision: ProjectMappingRevision,
        actor: &str,
        reason: &str,
    ) -> Result<ProjectRepositoryMapping, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(status: ProjectMappingStatus) -> ProjectRepositoryMapping {
        ProjectRepositoryMapping {
            id: ProjectRepositoryMappingId::new(),
            linear_organization_id: "organization".into(),
            linear_team_id: "team".into(),
            linear_project_id: LinearProjectId::new("project").unwrap(),
            linear_project_name_snapshot: "Project".into(),
            github_repository: RepositoryName::new("owner/repository").unwrap(),
            repository_source_path: "/repositories/source".into(),
            git_common_directory: "/repositories/source/.git".into(),
            git_remote_url: "git@github.com:owner/repository.git".into(),
            default_branch: "main".into(),
            status,
            revision: ProjectMappingRevision::new(1).unwrap(),
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn routing_decision_table_fails_closed() {
        let enabled = mapping(ProjectMappingStatus::Enabled);
        let disabled = mapping(ProjectMappingStatus::Disabled);
        let cases = [
            (
                Vec::new(),
                MappingRepositoryHealth::Healthy,
                ProjectRoutingDecision::RepositoryUnmapped,
            ),
            (
                vec![disabled.clone()],
                MappingRepositoryHealth::Healthy,
                ProjectRoutingDecision::MappingDisabled,
            ),
            (
                vec![enabled.clone()],
                MappingRepositoryHealth::Stale,
                ProjectRoutingDecision::MappingStale,
            ),
            (
                vec![enabled.clone()],
                MappingRepositoryHealth::Unhealthy,
                ProjectRoutingDecision::RepositoryUnhealthy,
            ),
            (
                vec![enabled.clone(), enabled.clone()],
                MappingRepositoryHealth::Healthy,
                ProjectRoutingDecision::MappingAmbiguous,
            ),
        ];
        for (mappings, health, expected) in cases {
            assert_eq!(resolve_project_routing(&mappings, health), expected);
        }
        assert!(matches!(
            resolve_project_routing(&[enabled], MappingRepositoryHealth::Healthy),
            ProjectRoutingDecision::Mapped { .. }
        ));
    }
}
