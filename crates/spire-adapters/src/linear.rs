//! Read-only Linear GraphQL adapter.
//!
//! The SDK is constructed here so it remains the pinned provider boundary. The
//! narrow raw-query transport is intentional: it supplies request limits and
//! rate-limit diagnostics that the SDK's public client construction does not
//! currently expose. This module contains no Linear mutation operation.

use std::{collections::BTreeSet, env, path::PathBuf, time::Duration};

use lineark_sdk::Client as LinearSdkClient;
use reqwest::{Client, StatusCode, header};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use spire_application::{
    AuthenticationState, CanonicalIssuePage, CanonicalLinearIssue, CanonicalLinearProject,
    CanonicalLinearProjectPage, ExternalResult, LinearEstimateScale, LinearOnboardingDiscoveryPort,
    LinearProjectQuery, LinearProjectReadPort, LinearReadPort, LinearStateCategory,
    LinearTeamConfiguration, LinearTeamSummary, LinearWorkflowState, ProbeConfidence,
    RelevantIssueQuery, ServiceAuthenticationProbe, ServiceAuthenticationProbePort,
};
use spire_domain::{LinearIssueId, LinearProjectId};
use thiserror::Error;
use tracing::debug;

const ENDPOINT: &str = "https://api.linear.app/graphql";
const MAX_RESPONSE_BYTES: usize = 1_048_576;
const PAGE_SIZE: usize = 50;
const MAX_DISCOVERY_PAGES: usize = 20;

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
    endpoint: String,
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
        Self::from_token_with_endpoint(token, ENDPOINT)
    }

    /// Test-only transport seam used to exercise request construction against a
    /// local fixture server without changing the production endpoint.
    pub fn from_token_with_endpoint(
        token: String,
        endpoint: impl Into<String>,
    ) -> Result<Self, LinearAdapterError> {
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
            endpoint: endpoint.into(),
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
            .post(&self.endpoint)
            // A Linear personal API key is sent raw. Wrapping it in `Bearer`
            // makes Linear reject the request with 400, not 401.
            .header(header::AUTHORIZATION, &self.token)
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

    async fn project(
        &self,
        project_id: &str,
    ) -> Result<Option<CanonicalLinearProject>, LinearAdapterError> {
        let data = self
            .request(PROJECT_QUERY, json!({"id": project_id}))
            .await?;
        reject_graphql_errors(&data)?;
        normalize_project_fixture(
            data.pointer("/data/project")
                .cloned()
                .unwrap_or(Value::Null),
        )
    }

    async fn projects(
        &self,
        query: &LinearProjectQuery,
    ) -> Result<CanonicalLinearProjectPage, LinearAdapterError> {
        let data = self
            .request(
                PROJECTS_QUERY,
                json!({
                    "first": PAGE_SIZE,
                    "after": query.cursor,
                    "includeArchived": query.include_archived
                }),
            )
            .await?;
        reject_graphql_errors(&data)?;
        parse_project_page(
            data.pointer("/data/projects")
                .cloned()
                .unwrap_or(Value::Null),
        )
    }

    /// Verifies the current API key using the non-mutating `viewer` query.
    pub async fn verify_viewer(&self) -> Result<LinearViewerIdentity, LinearAdapterError> {
        normalize_viewer_probe(self.request(VIEWER_QUERY, json!({})).await?)
    }
}

impl LinearOnboardingDiscoveryPort for LinearReadAdapter {
    type Error = LinearAdapterError;

    async fn list_teams(&self) -> Result<Vec<LinearTeamSummary>, Self::Error> {
        let mut teams = Vec::new();
        let mut cursor: Option<String> = None;
        // Bounded so a provider that never clears `hasNextPage` cannot pin the
        // onboarding prompt in an unbounded loop.
        for _ in 0..MAX_DISCOVERY_PAGES {
            let data = self
                .request(TEAMS_QUERY, json!({"first": PAGE_SIZE, "after": cursor}))
                .await?;
            reject_graphql_errors(&data)?;
            let connection = data
                .pointer("/data/teams")
                .ok_or(LinearAdapterError::MalformedResponse)?;
            for node in connection
                .pointer("/nodes")
                .and_then(Value::as_array)
                .ok_or(LinearAdapterError::MalformedResponse)?
            {
                teams.push(parse_team(node)?);
            }
            if !connection
                .pointer("/pageInfo/hasNextPage")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Ok(teams);
            }
            cursor = connection
                .pointer("/pageInfo/endCursor")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if cursor.is_none() {
                return Err(LinearAdapterError::MalformedResponse);
            }
        }
        Err(LinearAdapterError::MalformedResponse)
    }

    async fn team_configuration(
        &self,
        team_id: &str,
    ) -> Result<ExternalResult<LinearTeamConfiguration>, Self::Error> {
        let data = self
            .request(TEAM_CONFIGURATION_QUERY, json!({"id": team_id}))
            .await?;
        reject_graphql_errors(&data)?;
        let team = data.pointer("/data/team").cloned().unwrap_or(Value::Null);
        // A team with more than a page of workflow states is beyond what
        // onboarding can present for confirmation; stop rather than truncate.
        if team
            .pointer("/states/pageInfo/hasNextPage")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(ExternalResult::Ambiguous {
                detail: "team has more workflow states than onboarding can confirm".to_owned(),
            });
        }
        Ok(match normalize_team_configuration(team)? {
            Some(configuration) => ExternalResult::Confirmed(configuration),
            None => ExternalResult::NotFound,
        })
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

impl LinearProjectReadPort for LinearReadAdapter {
    type Error = LinearAdapterError;

    async fn get_project(
        &self,
        id: &LinearProjectId,
    ) -> Result<ExternalResult<CanonicalLinearProject>, Self::Error> {
        Ok(match self.project(id.as_str()).await? {
            Some(project) => ExternalResult::Confirmed(project),
            None => ExternalResult::NotFound,
        })
    }

    async fn list_projects(
        &self,
        query: &LinearProjectQuery,
    ) -> Result<ExternalResult<CanonicalLinearProjectPage>, Self::Error> {
        Ok(ExternalResult::Confirmed(self.projects(query).await?))
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
    #[serde(rename = "archivedAt")]
    archived_at: Option<String>,
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

pub fn normalize_project_fixture(
    value: Value,
) -> Result<Option<CanonicalLinearProject>, LinearAdapterError> {
    if value.is_null() {
        return Ok(None);
    }
    let raw: RawProject =
        serde_json::from_value(value).map_err(|_| LinearAdapterError::MalformedResponse)?;
    if raw.name.trim().is_empty() || raw.name.len() > 256 {
        return Err(LinearAdapterError::MalformedResponse);
    }
    Ok(Some(CanonicalLinearProject {
        id: LinearProjectId::new(raw.id).map_err(|_| LinearAdapterError::MalformedResponse)?,
        name: raw.name,
        archived_at: raw.archived_at,
    }))
}

fn parse_project_page(value: Value) -> Result<CanonicalLinearProjectPage, LinearAdapterError> {
    let raw: RawConnection<Value> =
        serde_json::from_value(value).map_err(|_| LinearAdapterError::MalformedResponse)?;
    let projects = raw
        .nodes
        .into_iter()
        .map(|value| normalize_project_fixture(value)?.ok_or(LinearAdapterError::MalformedResponse))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CanonicalLinearProjectPage {
        projects,
        next_cursor: raw
            .page_info
            .has_next_page
            .then_some(raw.page_info.end_cursor)
            .flatten(),
    })
}

fn reject_graphql_errors(value: &Value) -> Result<(), LinearAdapterError> {
    if let Some(errors) = value.get("errors").and_then(Value::as_array) {
        return Err(classify_viewer_errors(errors));
    }
    Ok(())
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
const PROJECT_QUERY: &str =
    "query Project($id: String!) { project(id: $id) { id name archivedAt } }";
const PROJECTS_QUERY: &str = "query Projects($first: Int!, $after: String, $includeArchived: Boolean!) { projects(first: $first, after: $after, includeArchived: $includeArchived) { nodes { id name archivedAt } pageInfo { hasNextPage endCursor } } }";
const VIEWER_QUERY: &str = "query Viewer { viewer { id organization { id } } }";
const TEAMS_QUERY: &str = "query Teams($first: Int!, $after: String) { teams(first: $first, after: $after) { nodes { id key name } pageInfo { hasNextPage endCursor } } }";
const TEAM_CONFIGURATION_QUERY: &str = "query TeamConfiguration($id: String!) { team(id: $id) { id key name issueEstimationType issueEstimationAllowZero issueEstimationExtended states(first: 100) { nodes { id name type position } pageInfo { hasNextPage endCursor } } } }";

/// Point values Linear can attach to an issue for a given estimation type.
/// Zero is excluded because `ComplexityEstimate` rejects it; a team that allows
/// zero simply leaves those issues unestimated as far as Spire is concerned.
fn estimate_points(kind: &str, extended: bool) -> Vec<u8> {
    match kind {
        "exponential" if extended => vec![1, 2, 4, 8, 16],
        "exponential" => vec![1, 2, 4, 8],
        "fibonacci" if extended => vec![1, 2, 3, 5, 8, 13],
        "fibonacci" => vec![1, 2, 3, 5, 8],
        "linear" if extended => vec![1, 2, 3, 4, 5, 6, 7],
        "linear" => vec![1, 2, 3, 4, 5],
        "tShirt" => vec![1, 2, 3, 5, 8],
        _ => Vec::new(),
    }
}

fn state_category(kind: Option<&str>) -> LinearStateCategory {
    match kind {
        Some("triage") => LinearStateCategory::Triage,
        Some("backlog") => LinearStateCategory::Backlog,
        Some("unstarted") => LinearStateCategory::Unstarted,
        Some("started") => LinearStateCategory::Started,
        Some("completed") => LinearStateCategory::Completed,
        Some("canceled") => LinearStateCategory::Canceled,
        _ => LinearStateCategory::Unrecognized,
    }
}

fn parse_team(value: &Value) -> Result<LinearTeamSummary, LinearAdapterError> {
    let field = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or(LinearAdapterError::MalformedResponse)
    };
    Ok(LinearTeamSummary {
        id: field("id")?.to_owned(),
        key: field("key")?.to_owned(),
        name: field("name")?.to_owned(),
    })
}

/// Converts a captured `team` response into the onboarding contract. States keep
/// Linear's own ordering so a suggestion never depends on response order.
pub fn normalize_team_configuration(
    value: Value,
) -> Result<Option<LinearTeamConfiguration>, LinearAdapterError> {
    if value.is_null() {
        return Ok(None);
    }
    let team = parse_team(&value)?;
    let mut states = value
        .pointer("/states/nodes")
        .and_then(Value::as_array)
        .ok_or(LinearAdapterError::MalformedResponse)?
        .iter()
        .map(|state| {
            let id = state
                .get("id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or(LinearAdapterError::MalformedResponse)?;
            let name = state
                .get("name")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or(LinearAdapterError::MalformedResponse)?;
            Ok((
                state.get("position").and_then(Value::as_f64).unwrap_or(0.0),
                LinearWorkflowState {
                    id: id.to_owned(),
                    name: name.to_owned(),
                    category: state_category(state.get("type").and_then(Value::as_str)),
                },
            ))
        })
        .collect::<Result<Vec<_>, LinearAdapterError>>()?;
    states.sort_by(|left, right| left.0.total_cmp(&right.0));

    let kind = value
        .get("issueEstimationType")
        .and_then(Value::as_str)
        .unwrap_or("notUsed");
    let extended = value
        .get("issueEstimationExtended")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(Some(LinearTeamConfiguration {
        team,
        states: states.into_iter().map(|(_, state)| state).collect(),
        estimates: LinearEstimateScale {
            kind: kind.to_owned(),
            points: estimate_points(kind, extended),
        },
    }))
}

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
    fn project_fixtures_distinguish_rename_archive_missing_and_malformed() {
        let original = normalize_project_fixture(
            serde_json::json!({"id":"project-1","name":"Original","archivedAt":null}),
        )
        .unwrap()
        .unwrap();
        let renamed = normalize_project_fixture(
            serde_json::json!({"id":"project-1","name":"Renamed","archivedAt":null}),
        )
        .unwrap()
        .unwrap();
        let archived = normalize_project_fixture(
            serde_json::json!({"id":"project-1","name":"Renamed","archivedAt":"2026-07-01T00:00:00Z"}),
        )
        .unwrap()
        .unwrap();

        assert_eq!(original.id, renamed.id);
        assert_ne!(original.name, renamed.name);
        assert!(archived.is_archived());
        assert_eq!(normalize_project_fixture(Value::Null).unwrap(), None);
        assert!(normalize_project_fixture(serde_json::json!({"id":"","name":""})).is_err());
    }

    #[test]
    fn team_configuration_orders_states_by_position_and_normalizes_categories() {
        let configuration = normalize_team_configuration(serde_json::json!({
            "id": "team-1",
            "key": "SPI",
            "name": "Spire",
            "issueEstimationType": "fibonacci",
            "issueEstimationAllowZero": true,
            "issueEstimationExtended": false,
            "states": {"nodes": [
                {"id": "done", "name": "Done", "type": "completed", "position": 4.0},
                {"id": "todo", "name": "Todo", "type": "unstarted", "position": 1.0},
                {"id": "doing", "name": "In Progress", "type": "started", "position": 2.0},
                {"id": "odd", "name": "Parked", "type": "invented", "position": 3.0}
            ], "pageInfo": {"hasNextPage": false, "endCursor": null}}
        }))
        .unwrap()
        .unwrap();

        assert_eq!(
            configuration
                .states
                .iter()
                .map(|state| state.id.as_str())
                .collect::<Vec<_>>(),
            ["todo", "doing", "odd", "done"]
        );
        assert_eq!(
            configuration.states[2].category,
            LinearStateCategory::Unrecognized
        );
        // Zero is never offered even when the team allows it.
        assert_eq!(configuration.estimates.points, [1, 2, 3, 5, 8]);
    }

    #[test]
    fn a_team_without_estimates_reports_an_empty_scale() {
        let configuration = normalize_team_configuration(serde_json::json!({
            "id": "team-1", "key": "SPI", "name": "Spire",
            "issueEstimationType": "notUsed",
            "states": {"nodes": [], "pageInfo": {"hasNextPage": false, "endCursor": null}}
        }))
        .unwrap()
        .unwrap();

        assert!(configuration.estimates.points.is_empty());
        assert_eq!(normalize_team_configuration(Value::Null).unwrap(), None);
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

    #[tokio::test]
    async fn transport_sends_a_linear_personal_key_without_bearer() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                let count = stream.read(&mut chunk).unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..count]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8(request).unwrap();
            let lower = request.to_ascii_lowercase();
            assert!(
                lower.contains("authorization: lin_api_test\r\n"),
                "{request}"
            );
            assert!(!lower.contains("authorization: bearer"), "{request}");
            let body = r#"{"data":{"viewer":{"id":"viewer-1","organization":{"id":"org-1"}}}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let adapter = LinearReadAdapter::from_token_with_endpoint(
            "lin_api_test".to_owned(),
            format!("http://{address}/graphql"),
        )
        .unwrap();
        let identity = adapter.verify_viewer().await.unwrap();
        assert_eq!(identity.viewer_id, "viewer-1");
        server.join().unwrap();
    }
}
