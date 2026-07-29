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
    AuthenticationState, CanonicalIssuePage, CanonicalLinearIssue, ExternalResult, LinearReadPort,
    ProbeConfidence, RelevantIssueQuery, ServiceAuthenticationProbe,
    ServiceAuthenticationProbePort,
};
use spire_domain::{LinearIssueId, LinearProjectId};
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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LinearAdapterError {
    #[error("Linear credential reference is invalid")]
    InvalidCredentialReference,
    #[error("Linear credential is unavailable")]
    CredentialUnavailable,
    #[error("Linear SDK client construction failed")]
    ClientConstruction,
    #[error("Linear authentication failed")]
    Authentication,
    #[error("Linear permission was denied")]
    PermissionDenied,
    #[error("Linear rate limit requires a pause of {retry_after_seconds:?} seconds")]
    RateLimited { retry_after_seconds: Option<u64> },
    #[error("Linear response exceeded the configured size limit")]
    ResponseTooLarge,
    #[error("Linear request failed with status {0}")]
    Http(StatusCode),
    #[error("Linear response was malformed")]
    MalformedResponse,
    #[error("Linear authentication response was ambiguous")]
    AmbiguousAuthentication,
    #[error("Linear network request failed")]
    Network,
}

/// Non-secret identity evidence returned after a successful Linear `viewer`
/// probe. The caller may retain this separately from the API key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinearViewerIdentity {
    pub viewer_id: String,
    pub organization_id: String,
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
        Self::from_token(token)
    }

    /// Builds the read-only adapter from a credential obtained by a managed
    /// secret-store adapter. Callers must not log or persist the token.
    pub fn from_token(token: String) -> Result<Self, LinearAdapterError> {
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
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(LinearAdapterError::Authentication);
        }
        if response.status() == StatusCode::FORBIDDEN {
            return Err(LinearAdapterError::PermissionDenied);
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

    /// Verifies the current API key using the non-mutating `viewer` query.
    pub async fn verify_viewer(&self) -> Result<LinearViewerIdentity, LinearAdapterError> {
        normalize_viewer_probe(self.request(VIEWER_QUERY, json!({})).await?)
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

#[allow(async_fn_in_trait)]
impl ServiceAuthenticationProbePort for LinearReadAdapter {
    type Error = LinearAdapterError;

    async fn probe_service(
        &self,
        service: &str,
    ) -> Result<ServiceAuthenticationProbe, Self::Error> {
        let (state, identity, confidence, remediation) = match self.verify_viewer().await {
            Ok(identity) => (
                AuthenticationState::Authenticated,
                Some(format!(
                    "viewer:{} organization:{}",
                    identity.viewer_id, identity.organization_id
                )),
                ProbeConfidence::Confirmed,
                None,
            ),
            Err(LinearAdapterError::Authentication) => (
                AuthenticationState::Expired,
                None,
                ProbeConfidence::Confirmed,
                Some("run spire auth rotate linear".into()),
            ),
            Err(LinearAdapterError::PermissionDenied) => (
                AuthenticationState::PermissionDenied,
                None,
                ProbeConfidence::Confirmed,
                Some("replace the Linear API key with one authorized for the configured workspace".into()),
            ),
            Err(LinearAdapterError::RateLimited { .. } | LinearAdapterError::Network) => (
                AuthenticationState::Unavailable,
                None,
                ProbeConfidence::Inferred,
                Some("retry the Linear authentication probe after provider recovery".into()),
            ),
            Err(
                LinearAdapterError::AmbiguousAuthentication
                | LinearAdapterError::MalformedResponse
                | LinearAdapterError::ResponseTooLarge
                | LinearAdapterError::Http(_)
                | LinearAdapterError::ClientConstruction
                | LinearAdapterError::CredentialUnavailable
                | LinearAdapterError::InvalidCredentialReference,
            ) => (
                AuthenticationState::Ambiguous,
                None,
                ProbeConfidence::Unknown,
                Some("update the captured Linear authentication fixture before trusting this response".into()),
            ),
        };
        Ok(ServiceAuthenticationProbe {
            service: service.into(),
            state,
            identity,
            expires_at: None,
            permissions: Vec::new(),
            missing_permissions: Vec::new(),
            confidence,
            remediation,
        })
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
    project: Option<RawProject>,
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
struct RawProject {
    id: String,
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
    let project_id = raw
        .project
        .as_ref()
        .map(|project| LinearProjectId::new(project.id.clone()))
        .transpose()
        .map_err(|_| LinearAdapterError::MalformedResponse)?;
    let project_name_snapshot = raw.project.map(|project| project.name);
    let acceptance_criteria = description
        .as_deref()
        .filter(|text| text.to_ascii_lowercase().contains("acceptance criteria"))
        .map(str::to_owned);
    let revision = CanonicalLinearIssue::revision(
        &raw.updated_at,
        description.as_deref().unwrap_or(""),
        project_id.as_ref(),
        project_name_snapshot.as_deref(),
    );
    Ok(Some(CanonicalLinearIssue {
        id: LinearIssueId::new(raw.id).map_err(|_| LinearAdapterError::MalformedResponse)?,
        identifier: raw.identifier,
        team_id: raw.team.id,
        workflow_state_id: raw.state.id,
        project_id,
        project_name_snapshot,
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

/// Converts a captured `viewer` response into non-secret identity evidence.
/// Any undocumented provider error fails closed as ambiguous.
pub fn normalize_viewer_probe(value: Value) -> Result<LinearViewerIdentity, LinearAdapterError> {
    if let Some(errors) = value.get("errors").and_then(Value::as_array) {
        return Err(classify_viewer_errors(errors));
    }
    let viewer = value
        .pointer("/data/viewer")
        .cloned()
        .ok_or(LinearAdapterError::AmbiguousAuthentication)?;
    let viewer_id = viewer
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(LinearAdapterError::MalformedResponse)?;
    let organization_id = viewer
        .pointer("/organization/id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(LinearAdapterError::MalformedResponse)?;
    Ok(LinearViewerIdentity {
        viewer_id: viewer_id.to_owned(),
        organization_id: organization_id.to_owned(),
    })
}

fn classify_viewer_errors(errors: &[Value]) -> LinearAdapterError {
    let codes = errors
        .iter()
        .filter_map(|error| error.pointer("/extensions/code").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    if codes.iter().any(|code| {
        matches!(
            *code,
            "AUTHENTICATION_ERROR" | "UNAUTHENTICATED" | "INVALID_TOKEN"
        )
    }) {
        LinearAdapterError::Authentication
    } else if codes
        .iter()
        .any(|code| matches!(*code, "FORBIDDEN" | "PERMISSION_DENIED"))
    {
        LinearAdapterError::PermissionDenied
    } else if codes
        .iter()
        .any(|code| matches!(*code, "RATE_LIMITED" | "TOO_MANY_REQUESTS"))
    {
        LinearAdapterError::RateLimited {
            retry_after_seconds: None,
        }
    } else {
        LinearAdapterError::AmbiguousAuthentication
    }
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

const ISSUE_QUERY: &str = "query Issue($id: String!) { issue(id: $id) { id identifier team { id } state { id } project { id name } estimate priority labels { nodes { name } pageInfo { hasNextPage endCursor } } relations { nodes { relatedIssue { id } issue { id } } pageInfo { hasNextPage endCursor } } description assignee { id } creator { id } createdAt updatedAt } }";
const ISSUES_QUERY: &str = "query Issues($first: Int!, $after: String, $filter: IssueFilter) { issues(first: $first, after: $after, filter: $filter) { nodes { id identifier team { id } state { id } project { id name } estimate priority labels { nodes { name } pageInfo { hasNextPage endCursor } } relations { nodes { relatedIssue { id } issue { id } } pageInfo { hasNextPage endCursor } } description assignee { id } creator { id } createdAt updatedAt } pageInfo { hasNextPage endCursor } } }";
const VIEWER_QUERY: &str = "query Viewer { viewer { id organization { id } } }";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_optional_values_and_hashes_untrusted_description() {
        let issue = normalize_issue_fixture(serde_json::json!({"id":"issue-1","identifier":"SPI-1","team":{"id":"team"},"state":{"id":"ready"},"project":{"id":"project-1","name":"Project"},"estimate":null,"priority":null,"labels":{"nodes":[{"name":"type:bug"}],"pageInfo":{"hasNextPage":false,"endCursor":null}},"relations":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}},"description":"untrusted instruction","assignee":null,"creator":null,"createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-02T00:00:00Z"})).unwrap().unwrap();
        assert_eq!(issue.estimate, None);
        assert_eq!(issue.assignee_id, None);
        assert_ne!(issue.revision, issue.updated_at);
        assert_eq!(issue.project_id.unwrap().as_str(), "project-1");
    }

    #[test]
    fn viewer_probe_maps_captured_authentication_outcomes_without_secret_data() {
        let authenticated = normalize_viewer_probe(
            serde_json::json!({"data":{"viewer":{"id":"viewer","organization":{"id":"org"}}}}),
        )
        .unwrap();
        assert_eq!(authenticated.viewer_id, "viewer");
        assert_eq!(authenticated.organization_id, "org");

        for (code, expected) in [
            ("INVALID_TOKEN", LinearAdapterError::Authentication),
            ("FORBIDDEN", LinearAdapterError::PermissionDenied),
            (
                "RATE_LIMITED",
                LinearAdapterError::RateLimited {
                    retry_after_seconds: None,
                },
            ),
            ("UNDOCUMENTED", LinearAdapterError::AmbiguousAuthentication),
        ] {
            let response = serde_json::json!({"errors":[{"extensions":{"code":code}}]});
            assert_eq!(normalize_viewer_probe(response), Err(expected));
        }
    }
}
