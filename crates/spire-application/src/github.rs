//! SHA-bound GitHub facts and CI gate evaluation.
//!
//! This module deliberately contains no HTTP or database code. Adapters fetch
//! canonical facts and the application decides whether those facts may advance
//! a work item.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use spire_domain::{CommitSha, RepositoryName};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalPullRequest {
    pub repository: RepositoryName,
    pub number: u64,
    pub url: String,
    pub state: PullRequestState,
    pub is_draft: bool,
    pub base_branch: String,
    pub base_sha: CommitSha,
    pub head_branch: String,
    pub head_sha: CommitSha,
    pub mergeable: Option<bool>,
    pub author: String,
    pub updated_at_unix_seconds: i64,
}

impl CanonicalPullRequest {
    pub fn belongs_to(&self, repository: &RepositoryName, branch: &str) -> bool {
        &self.repository == repository && self.head_branch == branch
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pending,
    Succeeded,
    Failed,
    Cancelled,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckRun {
    pub name: String,
    pub head_sha: CommitSha,
    pub status: CheckStatus,
    pub details_url: Option<String>,
    pub completed_at_unix_seconds: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredCheckGate {
    Pending { missing: Vec<String> },
    Failed { failures: Vec<CheckRun> },
    Succeeded { evidence: Vec<CheckRun> },
    ConfigurationError { missing: Vec<String> },
}

/// Evaluates configured required checks for one exact commit. Check data for a
/// different SHA is intentionally ignored, including green data.
pub fn evaluate_required_checks(
    required_names: &[String],
    current_head: &CommitSha,
    checks: &[CheckRun],
) -> RequiredCheckGate {
    let required = required_names.iter().cloned().collect::<BTreeSet<_>>();
    if required.is_empty() {
        return RequiredCheckGate::ConfigurationError {
            missing: vec!["no required checks configured".to_owned()],
        };
    }
    let mut by_name = BTreeMap::<String, Vec<CheckRun>>::new();
    for check in checks
        .iter()
        .filter(|check| &check.head_sha == current_head)
    {
        by_name
            .entry(check.name.clone())
            .or_default()
            .push(check.clone());
    }
    let missing = required
        .iter()
        .filter(|name| !by_name.contains_key(*name))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return RequiredCheckGate::Pending { missing };
    }
    let selected = required
        .iter()
        .flat_map(|name| by_name[name].iter().cloned())
        .collect::<Vec<_>>();
    let failures = selected
        .iter()
        .filter(|check| matches!(check.status, CheckStatus::Failed | CheckStatus::Cancelled))
        .cloned()
        .collect::<Vec<_>>();
    if !failures.is_empty() {
        return RequiredCheckGate::Failed { failures };
    }
    if selected
        .iter()
        .any(|check| check.status != CheckStatus::Succeeded)
    {
        return RequiredCheckGate::Pending {
            missing: Vec::new(),
        };
    }
    RequiredCheckGate::Succeeded { evidence: selected }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sha(value: &str) -> CommitSha {
        CommitSha::new(value).unwrap()
    }
    fn check(name: &str, head: &str, status: CheckStatus) -> CheckRun {
        CheckRun {
            name: name.into(),
            head_sha: sha(head),
            status,
            details_url: None,
            completed_at_unix_seconds: None,
        }
    }
    #[test]
    fn old_green_evidence_cannot_pass_a_new_head() {
        assert!(matches!(
            evaluate_required_checks(
                &["test".into()],
                &sha("new"),
                &[check("test", "old", CheckStatus::Succeeded)]
            ),
            RequiredCheckGate::Pending { .. }
        ));
    }
    #[test]
    fn optional_failure_does_not_fail_required_ci() {
        assert!(matches!(
            evaluate_required_checks(
                &["test".into()],
                &sha("head"),
                &[
                    check("test", "head", CheckStatus::Succeeded),
                    check("optional", "head", CheckStatus::Failed)
                ]
            ),
            RequiredCheckGate::Succeeded { .. }
        ));
    }
    #[test]
    fn cancelled_required_check_is_a_failure() {
        assert!(matches!(
            evaluate_required_checks(
                &["test".into()],
                &sha("head"),
                &[check("test", "head", CheckStatus::Cancelled)]
            ),
            RequiredCheckGate::Failed { .. }
        ));
    }
}
