//! GitHub REST adapter. It exposes no merge or force-push operation.

use std::{collections::BTreeSet, time::Duration};

use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT},
};
use serde::Deserialize;
use spire_application::{
    CanonicalPullRequest, CheckRun, CheckStatus, ExternalResult, GitHubPort, IdempotencyKey,
    MergeState, PullRequestState,
};
use spire_domain::{CommitSha, RepositoryName};
use thiserror::Error;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const GITHUB_API: &str = "https://api.github.com";

#[derive(Debug, Error)]
pub enum GitHubAdapterError {
    #[error("repository is not allowlisted: {0}")]
    UnauthorizedRepository(String),
    #[error("invalid GitHub credential reference")]
    InvalidCredential,
    #[error("GitHub rate limit reached; retry after {retry_after_seconds:?} seconds")]
    RateLimited { retry_after_seconds: Option<u64> },
    #[error("GitHub request failed with status {0}")]
    UnexpectedStatus(StatusCode),
    #[error("GitHub transport error: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("GitHub response was invalid: {0}")]
    InvalidResponse(String),
}

/// Receives one short-lived installation token minted by `github_app`.
/// The token remains in memory and is replaced by reconstructing this adapter
/// after the provider refreshes it; it is never durable configuration.
#[derive(Clone)]
pub struct GitHubHttpAdapter {
    client: Client,
    token: String,
    allowed_repositories: BTreeSet<RepositoryName>,
    api_base: String,
}

impl GitHubHttpAdapter {
    pub fn new(
        installation_access_token: String,
        allowed_repositories: impl IntoIterator<Item = RepositoryName>,
        timeout: Duration,
    ) -> Result<Self, GitHubAdapterError> {
        if installation_access_token.trim().is_empty() {
            return Err(GitHubAdapterError::InvalidCredential);
        }
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("spire-orchestrator"));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        let client = Client::builder()
            .default_headers(headers)
            .timeout(timeout)
            .build()?;
        Ok(Self {
            client,
            token: installation_access_token,
            allowed_repositories: allowed_repositories.into_iter().collect(),
            api_base: GITHUB_API.into(),
        })
    }

    fn check_repository(&self, repository: &RepositoryName) -> Result<(), GitHubAdapterError> {
        if self.allowed_repositories.contains(repository) {
            Ok(())
        } else {
            Err(GitHubAdapterError::UnauthorizedRepository(
                repository.to_string(),
            ))
        }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}{}", self.api_base, path))
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
    }

    async fn response(
        &self,
        response: reqwest::Response,
    ) -> Result<Option<reqwest::Response>, GitHubAdapterError> {
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if response.status() == StatusCode::FORBIDDEN
            || response.status() == StatusCode::TOO_MANY_REQUESTS
        {
            let retry_after_seconds = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse().ok());
            return Err(GitHubAdapterError::RateLimited {
                retry_after_seconds,
            });
        }
        if !response.status().is_success() {
            return Err(GitHubAdapterError::UnexpectedStatus(response.status()));
        }
        Ok(Some(response))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GitHubReconciliationReport {
    pub inspected: usize,
    pub updated: usize,
    pub missing: usize,
    pub conflicts: usize,
}

/// Narrow correctness repair for already-owned work. It intentionally does not
/// list repositories or enumerate an organization.
pub struct GitHubReconciler<'a, P> {
    database: &'a crate::sqlite::SqliteDatabase,
    github: &'a P,
}

#[derive(Debug, Error)]
pub enum GitHubReconcileError<E> {
    #[error("GitHub request failed: {0}")]
    GitHub(E),
    #[error("GitHub reconciliation database error: {0}")]
    Database(#[from] crate::sqlite::SqliteAdapterError),
    #[error("invalid persisted repository name: {0}")]
    InvalidRepository(String),
}

impl<'a, P> GitHubReconciler<'a, P> {
    pub fn new(database: &'a crate::sqlite::SqliteDatabase, github: &'a P) -> Self {
        Self { database, github }
    }
}

impl<P: GitHubPort> GitHubReconciler<'_, P> {
    pub async fn reconcile_active_pull_requests(
        &self,
        now: i64,
    ) -> Result<GitHubReconciliationReport, GitHubReconcileError<P::Error>> {
        let work_items = self.database.active_pull_request_work_items().await?;
        let mut report = GitHubReconciliationReport {
            inspected: 0,
            updated: 0,
            missing: 0,
            conflicts: 0,
        };
        for (work_item_id, repository, number, branch) in work_items {
            report.inspected += 1;
            let repository_name = RepositoryName::new(repository.clone())
                .map_err(|error| GitHubReconcileError::InvalidRepository(error.to_string()))?;
            match self
                .github
                .get_pull_request(&repository_name, number)
                .await
                .map_err(GitHubReconcileError::GitHub)?
            {
                ExternalResult::Confirmed(pull_request) => {
                    let result = match pull_request.state {
                        PullRequestState::Open => {
                            self.database
                                .persist_canonical_pull_request(
                                    &work_item_id,
                                    &repository,
                                    &branch,
                                    &pull_request,
                                    now,
                                )
                                .await
                        }
                        PullRequestState::Closed | PullRequestState::Merged => {
                            self.database
                                .persist_terminal_pull_request(
                                    &work_item_id,
                                    &repository,
                                    &branch,
                                    &pull_request,
                                    now,
                                )
                                .await
                        }
                    }?;
                    match result {
                        crate::sqlite::PullRequestPersistence::Updated => report.updated += 1,
                        crate::sqlite::PullRequestPersistence::IgnoredRepositoryOrBranch
                        | crate::sqlite::PullRequestPersistence::StaleWorkItem => {
                            report.conflicts += 1
                        }
                    }
                }
                ExternalResult::NotFound => report.missing += 1,
                ExternalResult::Ambiguous { .. } => report.conflicts += 1,
            }
        }
        Ok(report)
    }
}

#[allow(async_fn_in_trait)]
impl GitHubPort for GitHubHttpAdapter {
    type Error = GitHubAdapterError;

    async fn required_checks(
        &self,
        repository: &RepositoryName,
        head_sha: &CommitSha,
    ) -> Result<ExternalResult<Vec<CheckRun>>, Self::Error> {
        self.check_repository(repository)?;
        let path = format!("/repos/{repository}/commits/{head_sha}/check-runs?per_page=100");
        let Some(response) = self
            .response(self.request(reqwest::Method::GET, &path).send().await?)
            .await?
        else {
            return Ok(ExternalResult::NotFound);
        };
        let checks = response
            .json::<CheckRunsResponse>()
            .await?
            .check_runs
            .into_iter()
            .map(|check| check.into_check_run(head_sha.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ExternalResult::Confirmed(checks))
    }

    async fn get_pull_request(
        &self,
        repository: &RepositoryName,
        number: u64,
    ) -> Result<ExternalResult<CanonicalPullRequest>, Self::Error> {
        self.check_repository(repository)?;
        let path = format!("/repos/{repository}/pulls/{number}");
        let Some(response) = self
            .response(self.request(reqwest::Method::GET, &path).send().await?)
            .await?
        else {
            return Ok(ExternalResult::NotFound);
        };
        Ok(ExternalResult::Confirmed(
            response.json::<PullRequestResponse>().await?.try_into()?,
        ))
    }

    async fn find_pull_request_by_branch(
        &self,
        repository: &RepositoryName,
        branch: &str,
    ) -> Result<ExternalResult<CanonicalPullRequest>, Self::Error> {
        self.check_repository(repository)?;
        let owner =
            repository.as_str().split('/').next().ok_or_else(|| {
                GitHubAdapterError::InvalidResponse("repository lacks owner".into())
            })?;
        let path = format!(
            "/repos/{repository}/pulls?state=all&head={owner}:{}&per_page=100",
            urlencoding::encode(branch)
        );
        let Some(response) = self
            .response(self.request(reqwest::Method::GET, &path).send().await?)
            .await?
        else {
            return Ok(ExternalResult::NotFound);
        };
        let matches = response
            .json::<Vec<PullRequestResponse>>()
            .await?
            .into_iter()
            .filter(|pr| pr.head.r#ref == branch)
            .map(TryInto::try_into)
            .collect::<Result<Vec<CanonicalPullRequest>, _>>()?;
        match matches.len() {
            0 => Ok(ExternalResult::NotFound),
            1 => Ok(ExternalResult::Confirmed(
                matches.into_iter().next().expect("one match"),
            )),
            _ => Ok(ExternalResult::Ambiguous {
                detail: "multiple pull requests share the orchestrated branch".into(),
            }),
        }
    }

    async fn merge_state(
        &self,
        repository: &RepositoryName,
        number: u64,
    ) -> Result<ExternalResult<MergeState>, Self::Error> {
        match self.get_pull_request(repository, number).await? {
            ExternalResult::Confirmed(pr) => Ok(ExternalResult::Confirmed(match pr.state {
                PullRequestState::Open => MergeState::Open,
                PullRequestState::Closed => MergeState::Closed,
                PullRequestState::Merged => MergeState::Merged,
            })),
            ExternalResult::NotFound => Ok(ExternalResult::NotFound),
            ExternalResult::Ambiguous { detail } => Ok(ExternalResult::Ambiguous { detail }),
        }
    }

    async fn post_review_summary(
        &self,
        repository: &RepositoryName,
        number: u64,
        idempotency_key: &IdempotencyKey,
        body: &str,
    ) -> Result<ExternalResult<spire_application::PublishedComment>, Self::Error> {
        self.check_repository(repository)?;
        let path = format!("/repos/{repository}/issues/{number}/comments");
        let response = self
            .request(reqwest::Method::POST, &path)
            .header("x-spire-idempotency-key", &idempotency_key.0)
            .json(&serde_json::json!({"body": body}))
            .send()
            .await?;
        let Some(response) = self.response(response).await? else {
            return Ok(ExternalResult::NotFound);
        };
        let comment = response.json::<CommentResponse>().await?;
        Ok(ExternalResult::Confirmed(
            spire_application::PublishedComment {
                external_id: comment.id.to_string(),
                already_present: false,
            },
        ))
    }
}

#[derive(Deserialize)]
struct CheckRunsResponse {
    check_runs: Vec<CheckRunResponse>,
}
#[derive(Deserialize)]
struct CommentResponse {
    id: u64,
}
#[derive(Deserialize)]
struct CheckRunResponse {
    name: String,
    status: String,
    conclusion: Option<String>,
    details_url: Option<String>,
    completed_at: Option<String>,
}
impl CheckRunResponse {
    fn into_check_run(self, head_sha: CommitSha) -> Result<CheckRun, GitHubAdapterError> {
        let status = match (self.status.as_str(), self.conclusion.as_deref()) {
            ("completed", Some("success" | "neutral" | "skipped")) => CheckStatus::Succeeded,
            ("completed", Some("cancelled" | "timed_out" | "action_required")) => {
                CheckStatus::Cancelled
            }
            ("completed", Some(_)) => CheckStatus::Failed,
            _ => CheckStatus::Pending,
        };
        Ok(CheckRun {
            name: self.name,
            head_sha,
            status,
            details_url: self.details_url,
            completed_at_unix_seconds: parse_time(self.completed_at)?,
        })
    }
}

#[derive(Deserialize)]
struct PullRequestResponse {
    number: u64,
    html_url: String,
    state: String,
    draft: bool,
    merged_at: Option<String>,
    mergeable: Option<bool>,
    updated_at: String,
    user: User,
    base: Ref,
    head: Ref,
}
#[derive(Deserialize)]
struct User {
    login: String,
}
#[derive(Deserialize)]
struct Ref {
    #[serde(rename = "ref")]
    r#ref: String,
    sha: String,
    repo: Option<Repo>,
}
#[derive(Deserialize)]
struct Repo {
    full_name: String,
}
impl TryFrom<PullRequestResponse> for CanonicalPullRequest {
    type Error = GitHubAdapterError;
    fn try_from(value: PullRequestResponse) -> Result<Self, Self::Error> {
        let repository = value
            .head
            .repo
            .or(value.base.repo)
            .ok_or_else(|| {
                GitHubAdapterError::InvalidResponse("pull request has no repository".into())
            })?
            .full_name;
        Ok(Self {
            repository: RepositoryName::new(repository)
                .map_err(|error| GitHubAdapterError::InvalidResponse(error.to_string()))?,
            number: value.number,
            url: value.html_url,
            state: if value.merged_at.is_some() {
                PullRequestState::Merged
            } else if value.state == "open" {
                PullRequestState::Open
            } else {
                PullRequestState::Closed
            },
            is_draft: value.draft,
            base_branch: value.base.r#ref,
            base_sha: CommitSha::new(value.base.sha)
                .map_err(|error| GitHubAdapterError::InvalidResponse(error.to_string()))?,
            head_branch: value.head.r#ref,
            head_sha: CommitSha::new(value.head.sha)
                .map_err(|error| GitHubAdapterError::InvalidResponse(error.to_string()))?,
            mergeable: value.mergeable,
            author: value.user.login,
            updated_at_unix_seconds: parse_time(Some(value.updated_at))?.unwrap_or_default(),
        })
    }
}
fn parse_time(value: Option<String>) -> Result<Option<i64>, GitHubAdapterError> {
    value
        .map(|value| {
            OffsetDateTime::parse(&value, &Rfc3339)
                .map(|time| time.unix_timestamp())
                .map_err(|error| GitHubAdapterError::InvalidResponse(error.to_string()))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_check_normalization_preserves_failure_classes() {
        let head = CommitSha::new("head").unwrap();
        let failed = CheckRunResponse {
            name: "test".into(),
            status: "completed".into(),
            conclusion: Some("failure".into()),
            details_url: Some("https://example.test/check".into()),
            completed_at: Some("2026-07-29T00:00:00Z".into()),
        }
        .into_check_run(head.clone())
        .unwrap();
        let cancelled = CheckRunResponse {
            name: "test".into(),
            status: "completed".into(),
            conclusion: Some("cancelled".into()),
            details_url: None,
            completed_at: None,
        }
        .into_check_run(head)
        .unwrap();
        assert_eq!(failed.status, CheckStatus::Failed);
        assert_eq!(cancelled.status, CheckStatus::Cancelled);
    }
}
