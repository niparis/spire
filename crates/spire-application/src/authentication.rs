//! Authentication and diagnostics contracts.
//!
//! These values deliberately carry capability evidence and remediation only.
//! Secret values and provider-specific response bodies stay in adapters.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationState {
    Configured,
    Authenticated,
    Expired,
    PermissionDenied,
    Unavailable,
    Ambiguous,
    Unsupported,
}

impl AuthenticationState {
    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Configured | Self::Authenticated)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticFinding {
    pub code: String,
    pub required: bool,
    pub state: AuthenticationState,
    pub severity: DiagnosticSeverity,
    pub summary: String,
    pub remediation: Option<String>,
}

impl DiagnosticFinding {
    pub fn required(
        code: impl Into<String>,
        state: AuthenticationState,
        summary: impl Into<String>,
        remediation: Option<String>,
    ) -> Self {
        let severity = if state.is_ready() {
            DiagnosticSeverity::Info
        } else {
            DiagnosticSeverity::Error
        };
        Self {
            code: code.into(),
            required: true,
            state,
            severity,
            summary: summary.into(),
            remediation,
        }
    }

    pub fn required_authentication(
        code: impl Into<String>,
        state: AuthenticationState,
        summary: impl Into<String>,
        remediation: Option<String>,
    ) -> Self {
        Self {
            code: code.into(),
            required: true,
            state,
            severity: if state == AuthenticationState::Authenticated {
                DiagnosticSeverity::Info
            } else {
                DiagnosticSeverity::Error
            },
            summary: summary.into(),
            remediation,
        }
    }

    pub fn optional(
        code: impl Into<String>,
        state: AuthenticationState,
        severity: DiagnosticSeverity,
        summary: impl Into<String>,
        remediation: Option<String>,
    ) -> Self {
        Self {
            code: code.into(),
            required: false,
            state,
            severity,
            summary: summary.into(),
            remediation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticReport {
    pub ready: bool,
    pub findings: Vec<DiagnosticFinding>,
}

impl DiagnosticReport {
    /// Aggregates adapter observations without doing filesystem, process, or
    /// provider work. Required ambiguous states always fail closed.
    pub fn from_findings(findings: Vec<DiagnosticFinding>) -> Self {
        let ready = findings
            .iter()
            .filter(|finding| finding.required)
            .all(|finding| finding.severity != DiagnosticSeverity::Error);
        Self { ready, findings }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeConfidence {
    Confirmed,
    Inferred,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServiceAuthenticationProbe {
    pub service: String,
    pub state: AuthenticationState,
    pub identity: Option<String>,
    pub expires_at: Option<String>,
    pub permissions: Vec<String>,
    pub missing_permissions: Vec<String>,
    pub confidence: ProbeConfidence,
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HarnessProbe {
    pub harness: String,
    pub executable: String,
    pub version: Option<String>,
    pub state: AuthenticationState,
    pub supported_models: Vec<String>,
    pub supported_efforts: Vec<String>,
    pub confidence: ProbeConfidence,
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GitTransportProbe {
    pub repository_path: String,
    pub remote_name: Option<String>,
    pub remote_url: Option<String>,
    pub canonical_repository: Option<String>,
    pub default_branch: Option<String>,
    pub fetch_state: AuthenticationState,
    pub push_state: AuthenticationState,
    pub ephemeral_agent_risk: bool,
    pub confidence: ProbeConfidence,
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServiceContextProbe {
    pub unit_installed: bool,
    pub unit_active: bool,
    pub lingering_enabled: bool,
    pub runtime_user: String,
    pub ssh_agent_available: bool,
    pub state: AuthenticationState,
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedSecret {
    LinearApiKey,
    GitHubAppPrivateKey,
    GitHubWebhookSecret,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct AuthenticationMetadata {
    pub linear: Option<LinearAuthenticationMetadata>,
    pub github: Option<GitHubAuthenticationMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct LinearAuthenticationMetadata {
    pub viewer_id: String,
    pub organization_id: String,
    pub verified_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct GitHubAuthenticationMetadata {
    pub app_id: u64,
    pub app_slug: String,
    pub client_id: String,
    pub html_url: String,
    pub verified_at_unix: u64,
}

impl ManagedSecret {
    pub const fn key(self) -> &'static str {
        match self {
            Self::LinearApiKey => "LINEAR_API_KEY",
            Self::GitHubAppPrivateKey => "GITHUB_APP_PRIVATE_KEY",
            Self::GitHubWebhookSecret => "GITHUB_WEBHOOK_SECRET",
        }
    }
}

/// An application-owned lifecycle boundary. Implementations must not expose a
/// stored value through this port or include one in an error.
pub trait SecretStorePort {
    type Error;

    fn status(&self, secret: ManagedSecret) -> Result<AuthenticationState, Self::Error>;
    fn replace(&self, secret: ManagedSecret, value: SecretInput) -> Result<(), Self::Error>;
    fn remove(&self, secret: ManagedSecret) -> Result<(), Self::Error>;
}

pub trait AuthenticationMetadataStorePort {
    type Error;

    fn load(&self) -> Result<AuthenticationMetadata, Self::Error>;
    fn store(&self, metadata: &AuthenticationMetadata) -> Result<(), Self::Error>;
}

pub trait SecretPromptPort {
    type Error;

    fn prompt_secret(&self, prompt: &str) -> Result<SecretInput, Self::Error>;
}

/// A write-only secret input. Its `Debug` implementation is intentionally
/// redacted so that ordinary error paths and test failures cannot expose it.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretInput(String);

impl SecretInput {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretInput([redacted])")
    }
}

#[allow(async_fn_in_trait)]
pub trait ServiceAuthenticationProbePort {
    type Error;

    async fn probe_service(&self, service: &str)
    -> Result<ServiceAuthenticationProbe, Self::Error>;
}

pub trait HarnessProbePort {
    type Error;

    fn probe_harness(&self, harness: &str) -> Result<HarnessProbe, Self::Error>;
}

pub trait GitTransportProbePort {
    type Error;

    fn probe_git_transport(&self) -> Result<GitTransportProbe, Self::Error>;
}

pub trait ServiceContextProbePort {
    type Error;

    fn probe_service_context(&self) -> Result<ServiceContextProbe, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_provider_response_fails_closed() {
        let report = DiagnosticReport::from_findings(vec![DiagnosticFinding::required(
            "SPIRE-AUTH-001",
            AuthenticationState::Ambiguous,
            "provider returned an undocumented response",
            Some("run spire auth status after updating the provider fixture".into()),
        )]);

        assert!(!report.ready);
        assert_eq!(report.findings[0].severity, DiagnosticSeverity::Error);
    }

    #[test]
    fn required_state_table_controls_exit_readiness() {
        for (state, ready) in [
            (AuthenticationState::Configured, true),
            (AuthenticationState::Authenticated, true),
            (AuthenticationState::Expired, false),
            (AuthenticationState::PermissionDenied, false),
            (AuthenticationState::Unavailable, false),
            (AuthenticationState::Ambiguous, false),
            (AuthenticationState::Unsupported, false),
        ] {
            let report = DiagnosticReport::from_findings(vec![DiagnosticFinding::required(
                "SPIRE-AUTH-002",
                state,
                "table test",
                None,
            )]);
            assert_eq!(report.ready, ready, "{state:?}");
        }
    }

    #[test]
    fn configured_secret_is_not_authenticated_service_evidence() {
        let report =
            DiagnosticReport::from_findings(vec![DiagnosticFinding::required_authentication(
                "SPIRE-AUTH-003",
                AuthenticationState::Configured,
                "credential exists but has not been verified",
                None,
            )]);
        assert!(!report.ready);
    }

    #[test]
    fn secret_input_debug_is_redacted() {
        let secret = SecretInput::new("SPIRE_SECRET_SENTINEL".into());
        assert!(!format!("{secret:?}").contains("SPIRE_SECRET_SENTINEL"));
    }

    #[test]
    fn github_app_secret_parts_have_distinct_store_keys() {
        assert_eq!(
            ManagedSecret::GitHubAppPrivateKey.key(),
            "GITHUB_APP_PRIVATE_KEY"
        );
        assert_eq!(
            ManagedSecret::GitHubWebhookSecret.key(),
            "GITHUB_WEBHOOK_SECRET"
        );
    }

    #[test]
    fn diagnostic_json_golden_reports_cover_ready_degraded_and_blocked() {
        let cases = [
            (
                DiagnosticReport::from_findings(vec![DiagnosticFinding::required(
                    "SPIRE-DB-001",
                    AuthenticationState::Authenticated,
                    "database integrity is valid",
                    None,
                )]),
                r#"{"ready":true,"findings":[{"code":"SPIRE-DB-001","required":true,"state":"authenticated","severity":"info","summary":"database integrity is valid","remediation":null}]}"#,
            ),
            (
                DiagnosticReport::from_findings(vec![
                    DiagnosticFinding::required(
                        "SPIRE-DB-001",
                        AuthenticationState::Authenticated,
                        "database integrity is valid",
                        None,
                    ),
                    DiagnosticFinding::optional(
                        "SPIRE-GIT-002",
                        AuthenticationState::Ambiguous,
                        DiagnosticSeverity::Warning,
                        "push authority is unverified",
                        Some("verify push authority before enabling writes".into()),
                    ),
                ]),
                r#"{"ready":true,"findings":[{"code":"SPIRE-DB-001","required":true,"state":"authenticated","severity":"info","summary":"database integrity is valid","remediation":null},{"code":"SPIRE-GIT-002","required":false,"state":"ambiguous","severity":"warning","summary":"push authority is unverified","remediation":"verify push authority before enabling writes"}]}"#,
            ),
            (
                DiagnosticReport::from_findings(vec![DiagnosticFinding::required(
                    "SPIRE-SVC-001",
                    AuthenticationState::Ambiguous,
                    "service context is ambiguous",
                    Some("run spire doctor from the user service context".into()),
                )]),
                r#"{"ready":false,"findings":[{"code":"SPIRE-SVC-001","required":true,"state":"ambiguous","severity":"error","summary":"service context is ambiguous","remediation":"run spire doctor from the user service context"}]}"#,
            ),
        ];

        for (report, expected) in cases {
            assert_eq!(serde_json::to_string(&report).unwrap(), expected);
        }
    }
}
