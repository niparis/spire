use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spire_domain::{
    ComplexityClass, LinearIssueId, LinearProjectId, ProjectMappingRevision,
    ProjectRepositoryMappingId, RepositoryName, RunRole,
};

use crate::ProjectRoutingDecision;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalLinearIssue {
    pub id: LinearIssueId,
    pub identifier: String,
    pub team_id: String,
    pub workflow_state_id: String,
    pub project_id: Option<LinearProjectId>,
    pub project_name_snapshot: Option<String>,
    pub estimate: Option<u8>,
    pub priority: Option<u8>,
    pub labels: BTreeSet<String>,
    pub blockers: Vec<LinearIssueId>,
    pub description: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub assignee_id: Option<String>,
    pub creator_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalLinearProject {
    pub id: LinearProjectId,
    pub name: String,
    pub archived_at: Option<String>,
}

impl CanonicalLinearProject {
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalLinearProjectPage {
    pub projects: Vec<CanonicalLinearProject>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearProjectQuery {
    pub cursor: Option<String>,
    pub include_archived: bool,
}

#[allow(async_fn_in_trait)]
pub trait LinearProjectReadPort {
    type Error;

    async fn get_project(
        &self,
        id: &LinearProjectId,
    ) -> Result<crate::ExternalResult<CanonicalLinearProject>, Self::Error>;

    async fn list_projects(
        &self,
        query: &LinearProjectQuery,
    ) -> Result<crate::ExternalResult<CanonicalLinearProjectPage>, Self::Error>;
}

impl CanonicalLinearIssue {
    pub fn revision(
        updated_at: &str,
        content: &str,
        project_id: Option<&LinearProjectId>,
        project_name_snapshot: Option<&str>,
    ) -> String {
        let digest = Sha256::digest(
            format!(
                "{content}\u{1f}{}\u{1f}{}",
                project_id.map(LinearProjectId::as_str).unwrap_or(""),
                project_name_snapshot.unwrap_or("")
            )
            .as_bytes(),
        );
        format!("{updated_at}:{}", hex::encode(digest))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryMapping {
    pub label: String,
    pub repository: RepositoryName,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EligibilityResult {
    Eligible {
        repository: RepositoryName,
        mapping_id: ProjectRepositoryMappingId,
        mapping_revision: ProjectMappingRevision,
        complexity: ComplexityClass,
    },
    Ineligible {
        reason: EligibilityReason,
        operator_detail: String,
    },
    WaitingForDependency {
        blockers: Vec<LinearIssueId>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EligibilityReason {
    NotReady,
    UnsupportedType,
    MissingAcceptanceCriteria,
    ComplexityMissing,
    ComplexityUnmapped,
    RepositoryUnmapped,
    MappingDisabled,
    MappingStale,
    MappingAmbiguous,
    RepositoryUnhealthy,
    AlreadyActive,
    LocallyTerminal,
    DispatchCoverageMissing,
}

#[derive(Debug, Clone)]
pub struct EligibilityInput<'a> {
    pub issue: &'a CanonicalLinearIssue,
    pub ready_state_id: &'a str,
    pub supported_type_labels: &'a BTreeSet<String>,
    pub project_routing: &'a ProjectRoutingDecision,
    pub complexity_mapping: &'a std::collections::BTreeMap<u8, ComplexityClass>,
    pub incomplete_blockers: &'a BTreeSet<LinearIssueId>,
    pub locally_active: bool,
    pub locally_terminal: bool,
    pub dispatch_covers_implementation_and_review: bool,
}

pub fn evaluate_eligibility(input: EligibilityInput<'_>) -> EligibilityResult {
    let issue = input.issue;
    if issue.workflow_state_id != input.ready_state_id {
        return ineligible(
            EligibilityReason::NotReady,
            "issue is not in the configured Ready-for-Agent workflow state",
        );
    }
    if !issue
        .labels
        .iter()
        .any(|label| input.supported_type_labels.contains(label))
    {
        return ineligible(
            EligibilityReason::UnsupportedType,
            "issue has no configured supported work-type label",
        );
    }
    if issue.labels.iter().any(|label| {
        matches!(
            label.as_str(),
            "type:architecture" | "type:adr" | "type:spike"
        )
    }) {
        return ineligible(
            EligibilityReason::UnsupportedType,
            "architecture, ADR, and spike work is excluded from automation",
        );
    }
    if issue
        .acceptance_criteria
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        return ineligible(
            EligibilityReason::MissingAcceptanceCriteria,
            "issue has no acceptance-criteria evidence",
        );
    }
    let Some(estimate) = issue.estimate else {
        return ineligible(
            EligibilityReason::ComplexityMissing,
            "issue has no Linear estimate",
        );
    };
    let Some(complexity) = input.complexity_mapping.get(&estimate).copied() else {
        return ineligible(
            EligibilityReason::ComplexityUnmapped,
            "issue estimate is not in the configured complexity mapping",
        );
    };
    let (mapping_id, mapping_revision, repository) = match input.project_routing {
        ProjectRoutingDecision::Mapped {
            mapping_id,
            mapping_revision,
            repository,
        } => (*mapping_id, *mapping_revision, repository.clone()),
        ProjectRoutingDecision::RepositoryUnmapped => {
            return ineligible(
                EligibilityReason::RepositoryUnmapped,
                "issue has no enabled durable project mapping",
            );
        }
        ProjectRoutingDecision::MappingDisabled => {
            return ineligible(
                EligibilityReason::MappingDisabled,
                "the durable project mapping is disabled",
            );
        }
        ProjectRoutingDecision::MappingStale => {
            return ineligible(
                EligibilityReason::MappingStale,
                "the durable project mapping has stale authority evidence",
            );
        }
        ProjectRoutingDecision::MappingAmbiguous => {
            return ineligible(
                EligibilityReason::MappingAmbiguous,
                "durable project routing is ambiguous",
            );
        }
        ProjectRoutingDecision::RepositoryUnhealthy => {
            return ineligible(
                EligibilityReason::RepositoryUnhealthy,
                "the mapped repository is unavailable or unhealthy",
            );
        }
    };
    let blockers = issue
        .blockers
        .iter()
        .filter(|id| input.incomplete_blockers.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    if !blockers.is_empty() {
        return EligibilityResult::WaitingForDependency { blockers };
    }
    if input.locally_active {
        return ineligible(
            EligibilityReason::AlreadyActive,
            "work item already has local active ownership",
        );
    }
    if input.locally_terminal {
        return ineligible(
            EligibilityReason::LocallyTerminal,
            "work item is locally terminal",
        );
    }
    if !input.dispatch_covers_implementation_and_review {
        return ineligible(
            EligibilityReason::DispatchCoverageMissing,
            "dispatch policy does not cover both implementation and review",
        );
    }
    EligibilityResult::Eligible {
        repository,
        mapping_id,
        mapping_revision,
        complexity,
    }
}

fn ineligible(reason: EligibilityReason, operator_detail: &str) -> EligibilityResult {
    EligibilityResult::Ineligible {
        reason,
        operator_detail: operator_detail.to_owned(),
    }
}

pub fn dispatch_is_covered(
    policy: &spire_domain::DispatchPolicy,
    capabilities: &spire_domain::HarnessCapabilityRegistry,
    complexity: ComplexityClass,
) -> bool {
    [RunRole::Implementation, RunRole::Review]
        .into_iter()
        .all(|role| {
            policy
                .evaluate(capabilities, role, complexity, &[], None)
                .is_ok()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn issue() -> CanonicalLinearIssue {
        CanonicalLinearIssue {
            id: LinearIssueId::new("issue-1").unwrap(),
            identifier: "SPI-1".into(),
            team_id: "team".into(),
            workflow_state_id: "ready".into(),
            project_id: Some(LinearProjectId::new("project-1").unwrap()),
            project_name_snapshot: Some("Project".into()),
            estimate: Some(2),
            priority: None,
            labels: ["type:bug".into(), "repo:spire".into()]
                .into_iter()
                .collect(),
            blockers: vec![],
            description: Some("untrusted".into()),
            acceptance_criteria: Some("works".into()),
            assignee_id: None,
            creator_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-02T00:00:00Z".into(),
            revision: CanonicalLinearIssue::revision(
                "2026-01-02T00:00:00Z",
                "untrusted",
                Some(&LinearProjectId::new("project-1").unwrap()),
                Some("Project"),
            ),
        }
    }

    #[test]
    fn eligibility_reports_each_individual_failure_without_defaults() {
        let issue = issue();
        let types = ["type:bug".into()].into_iter().collect();
        let routing = ProjectRoutingDecision::Mapped {
            mapping_id: ProjectRepositoryMappingId::new(),
            mapping_revision: ProjectMappingRevision::new(1).unwrap(),
            repository: RepositoryName::new("owner/spire").unwrap(),
        };
        let complexity = BTreeMap::from([(2, ComplexityClass::Medium)]);
        let result = evaluate_eligibility(EligibilityInput {
            issue: &issue,
            ready_state_id: "ready",
            supported_type_labels: &types,
            project_routing: &routing,
            complexity_mapping: &complexity,
            incomplete_blockers: &BTreeSet::new(),
            locally_active: false,
            locally_terminal: false,
            dispatch_covers_implementation_and_review: true,
        });
        assert!(matches!(
            result,
            EligibilityResult::Eligible {
                complexity: ComplexityClass::Medium,
                ..
            }
        ));
        let mut relabeled = issue.clone();
        relabeled.labels = ["type:bug".into(), "repo:hostile/override".into()]
            .into_iter()
            .collect();
        assert_eq!(
            evaluate_eligibility(EligibilityInput {
                issue: &relabeled,
                ready_state_id: "ready",
                supported_type_labels: &types,
                project_routing: &routing,
                complexity_mapping: &complexity,
                incomplete_blockers: &BTreeSet::new(),
                locally_active: false,
                locally_terminal: false,
                dispatch_covers_implementation_and_review: true,
            }),
            result
        );
        let mut missing = issue.clone();
        missing.estimate = None;
        assert!(matches!(
            evaluate_eligibility(EligibilityInput {
                issue: &missing,
                ready_state_id: "ready",
                supported_type_labels: &types,
                project_routing: &routing,
                complexity_mapping: &complexity,
                incomplete_blockers: &BTreeSet::new(),
                locally_active: false,
                locally_terminal: false,
                dispatch_covers_implementation_and_review: true
            }),
            EligibilityResult::Ineligible {
                reason: EligibilityReason::ComplexityMissing,
                ..
            }
        ));
    }
}
