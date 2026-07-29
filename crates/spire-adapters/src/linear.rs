//! Read-only Linear GraphQL adapter.
//!
//! The SDK is constructed here so it remains the pinned provider boundary. The
//! narrow raw-query transport is intentional: it supplies request limits and
//! rate-limit diagnostics that the SDK's public client construction does not
//! currently expose. This module contains no Linear mutation operation.

use std::{collections::BTreeSet, env, path::PathBuf, time::Duration};

use lineark_sdk::Client as LinearSdkClient;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use spire_application::{
    CanonicalIssuePage, CanonicalLinearIssue, ExternalResult, LinearReadPort, RelevantIssueQuery,
};
use spire_domain::LinearIssueId;
use thiserror::Error;
use tracing::debug;

const ENDPOINT: &str = "https://api.linear.app/graphql";
const MAX_RESPONSE_BYTES: usize = 1_048_576;
const PAGE_SIZE: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinearHealthDiagnostic {
    pub configured: bool,
    pub last_rate_limit_remaining: Option<u64>,
    pub retry_after_seconds: Option<u64>,
}

#[derive(Debug, Error)]
pub enum LinearAdapterError {
    #[error("Linear credential reference is invalid")]
    InvalidCredentialReference,
    #[error("Linear credential is unavailable")]
    CredentialUnavailable,
    #[error("Linear SDK client construction failed")]
    ClientConstruction,
    #[error("Linear authentication failed")]
    Authentication,
    #[error("Linear rate limit requires a pause of {retry_after_seconds:?} seconds")]
    RateLimited { retry_after_seconds: Option<u64> },
    #[error("Linear response exceeded the configured size limit")]
    ResponseTooLarge,
    #[error("Linear request failed with status {0}")]
    Http(StatusCode),
    #[error("Linear response was malformed")]
    MalformedResponse,
    #[error("Linear network request failed")]
    Network,
}

pub struct LinearReadAdapter {
    _sdk: LinearSdkClient,
    http: Client,
    token: String,
    diagnostic: std::sync::Mutex<LinearHealthDiagnostic>,
}

impl std::fmt::Debug for LinearReadAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LinearReadAdapter")
            .field("token", &"[redacted]")
            .finish_non_exhaustive()
    }
}

impl LinearReadAdapter {
    pub fn from_credential_reference(reference: &str) -> Result<Self, LinearAdapterError> {
        let token = load_credential(reference)?;
        let sdk = LinearSdkClient::from_token(token.clone())
            .map_err(|_| LinearAdapterError::ClientConstruction)?;
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("spire/0.1 linear-read-only")
            .build()
            .map_err(|_| LinearAdapterError::ClientConstruction)?;
        Ok(Self {
            _sdk: sdk,
            http,
            token,
            diagnostic: std::sync::Mutex::new(LinearHealthDiagnostic {
                configured: true,
                last_rate_limit_remaining: None,
                retry_after_seconds: None,
            }),
        })
    }

    pub fn health(&self) -> LinearHealthDiagnostic {
        self.diagnostic
            .lock()
            .expect("health lock is not poisoned")
            .clone()
    }

    async fn request(&self, query: &str, variables: Value) -> Result<Value, LinearAdapterError> {
        let response = self
            .http
            .post(ENDPOINT)
            .bearer_auth(&self.token)
            .json(&json!({"query": query, "variables": variables}))
            .send()
            .await
            .map_err(|_| LinearAdapterError::Network)?;
        let retry_after_seconds = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        let remaining = response
            .headers()
            .get("x-ratelimit-requests-remaining")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        if let Ok(mut diagnostic) = self.diagnostic.lock() {
            diagnostic.last_rate_limit_remaining = remaining;
            diagnostic.retry_after_seconds = retry_after_seconds;
        }
        if response.status() == StatusCode::UNAUTHORIZED
            || response.status() == StatusCode::FORBIDDEN
        {
            return Err(LinearAdapterError::Authentication);
        }
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            return Err(LinearAdapterError::RateLimited {
                retry_after_seconds,
            });
        }
        if !response.status().is_success() {
            return Err(LinearAdapterError::Http(response.status()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(LinearAdapterError::ResponseTooLarge);
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| LinearAdapterError::Network)?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(LinearAdapterError::ResponseTooLarge);
        }
        serde_json::from_slice(&bytes).map_err(|_| LinearAdapterError::MalformedResponse)
    }

    async fn issue(
        &self,
        issue_id: &str,
    ) -> Result<Option<CanonicalLinearIssue>, LinearAdapterError> {
        let data = self.request(ISSUE_QUERY, json!({"id": issue_id})).await?;
        normalize_issue_fixture(data.pointer("/data/issue").cloned().unwrap_or(Value::Null))
    }

    async fn issues(
        &self,
        query: &RelevantIssueQuery,
    ) -> Result<CanonicalIssuePage, LinearAdapterError> {
        let data = self.request(ISSUES_QUERY, json!({"first": PAGE_SIZE, "after": query.cursor, "filter": {"team": {"id": {"eq": query.team_id}}, "state": {"id": {"in": query.workflow_state_ids}}}})).await?;
        parse_page(data.pointer("/data/issues").cloned().unwrap_or(Value::Null))
    }
}

impl LinearReadPort for LinearReadAdapter {
    type Error = LinearAdapterError;

    async fn get_canonical_issue(
        &self,
        issue_id: &LinearIssueId,
    ) -> Result<ExternalResult<CanonicalLinearIssue>, Self::Error> {
        Ok(match self.issue(issue_id.as_str()).await? {
            Some(issue) => ExternalResult::Confirmed(issue),
            None => ExternalResult::NotFound,
        })
    }

    async fn find_canonical_issues(
        &self,
        query: &RelevantIssueQuery,
    ) -> Result<ExternalResult<CanonicalIssuePage>, Self::Error> {
        Ok(ExternalResult::Confirmed(self.issues(query).await?))
    }
}

/// Resolves an operator-provided credential reference without logging it.
///
/// The API process uses the same narrowly scoped mechanism for the Linear
/// webhook signing secret; credential material never leaves the caller.
pub fn load_credential(reference: &str) -> Result<String, LinearAdapterError> {
    if let Some(name) = reference.strip_prefix("env:") {
        return env::var(name).map_err(|_| LinearAdapterError::CredentialUnavailable);
    }
    if let Some(name) = reference.strip_prefix("systemd:credentials/") {
        let directory = env::var_os("CREDENTIALS_DIRECTORY")
            .ok_or(LinearAdapterError::CredentialUnavailable)?;
        return std::fs::read_to_string(PathBuf::from(directory).join(name))
            .map(|value| value.trim().to_owned())
            .map_err(|_| LinearAdapterError::CredentialUnavailable);
    }
    Err(LinearAdapterError::InvalidCredentialReference)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawIssue {
    id: String,
    identifier: String,
    team: RawId,
    state: RawId,
    estimate: Option<u8>,
    priority: Option<u8>,
    labels: RawConnection<RawName>,
    relations: RawConnection<RawRelation>,
    description: Option<String>,
    assignee: Option<RawId>,
    creator: Option<RawId>,
    created_at: String,
    updated_at: String,
}
#[derive(Deserialize)]
struct RawId {
    id: String,
}
#[derive(Deserialize)]
struct RawName {
    name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRelation {
    related_issue: Option<RawId>,
    issue: Option<RawId>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConnection<T> {
    nodes: Vec<T>,
    page_info: RawPageInfo,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

pub fn normalize_issue_fixture(
    value: Value,
) -> Result<Option<CanonicalLinearIssue>, LinearAdapterError> {
    if value.is_null() {
        return Ok(None);
    }
    let raw: RawIssue =
        serde_json::from_value(value).map_err(|_| LinearAdapterError::MalformedResponse)?;
    let blockers = raw
        .relations
        .nodes
        .into_iter()
        .filter_map(|relation| relation.related_issue.or(relation.issue))
        .map(|issue| {
            LinearIssueId::new(issue.id).map_err(|_| LinearAdapterError::MalformedResponse)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let description = raw.description;
    let acceptance_criteria = description
        .as_deref()
        .filter(|text| text.to_ascii_lowercase().contains("acceptance criteria"))
        .map(str::to_owned);
    let revision =
        CanonicalLinearIssue::revision(&raw.updated_at, description.as_deref().unwrap_or(""));
    Ok(Some(CanonicalLinearIssue {
        id: LinearIssueId::new(raw.id).map_err(|_| LinearAdapterError::MalformedResponse)?,
        identifier: raw.identifier,
        team_id: raw.team.id,
        workflow_state_id: raw.state.id,
        estimate: raw.estimate,
        priority: raw.priority,
        labels: raw
            .labels
            .nodes
            .into_iter()
            .map(|label| label.name)
            .collect::<BTreeSet<_>>(),
        blockers,
        description,
        acceptance_criteria,
        assignee_id: raw.assignee.map(|id| id.id),
        creator_id: raw.creator.map(|id| id.id),
        created_at: raw.created_at,
        updated_at: raw.updated_at,
        revision,
    }))
}

fn parse_page(value: Value) -> Result<CanonicalIssuePage, LinearAdapterError> {
    let raw: RawConnection<Value> =
        serde_json::from_value(value).map_err(|_| LinearAdapterError::MalformedResponse)?;
    let mut issues = Vec::with_capacity(raw.nodes.len());
    for value in raw.nodes {
        if let Some(issue) = normalize_issue_fixture(value)? {
            issues.push(issue);
        }
    }
    debug!(issue_count = issues.len(), "normalized Linear issue page");
    Ok(CanonicalIssuePage {
        issues,
        next_cursor: raw
            .page_info
            .has_next_page
            .then_some(raw.page_info.end_cursor)
            .flatten(),
    })
}

const ISSUE_QUERY: &str = "query Issue($id: String!) { issue(id: $id) { id identifier team { id } state { id } estimate priority labels { nodes { name } pageInfo { hasNextPage endCursor } } relations { nodes { relatedIssue { id } issue { id } } pageInfo { hasNextPage endCursor } } description assignee { id } creator { id } createdAt updatedAt } }";
const ISSUES_QUERY: &str = "query Issues($first: Int!, $after: String, $filter: IssueFilter) { issues(first: $first, after: $after, filter: $filter) { nodes { id identifier team { id } state { id } estimate priority labels { nodes { name } pageInfo { hasNextPage endCursor } } relations { nodes { relatedIssue { id } issue { id } } pageInfo { hasNextPage endCursor } } description assignee { id } creator { id } createdAt updatedAt } pageInfo { hasNextPage endCursor } } }";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_optional_values_and_hashes_untrusted_description() {
        let issue = normalize_issue_fixture(serde_json::json!({"id":"issue-1","identifier":"SPI-1","team":{"id":"team"},"state":{"id":"ready"},"estimate":null,"priority":null,"labels":{"nodes":[{"name":"type:bug"}],"pageInfo":{"hasNextPage":false,"endCursor":null}},"relations":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}},"description":"untrusted instruction","assignee":null,"creator":null,"createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-02T00:00:00Z"})).unwrap().unwrap();
        assert_eq!(issue.estimate, None);
        assert_eq!(issue.assignee_id, None);
        assert_ne!(issue.revision, issue.updated_at);
    }
}
