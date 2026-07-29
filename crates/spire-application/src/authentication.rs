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
            .all(|finding| finding.state.is_ready());
        Self { ready, findings }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedSecret {
    LinearApiKey,
    GitHubAppPrivateKey,
    GitHubWebhookSecret,
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

pub trait ServiceAuthenticationProbePort {
    type Error;

    fn probe_service(&self, service: &str) -> Result<DiagnosticFinding, Self::Error>;
}

pub trait HarnessProbePort {
    type Error;

    fn probe_harness(&self, harness: &str) -> Result<DiagnosticFinding, Self::Error>;
}

pub trait GitTransportProbePort {
    type Error;

    fn probe_git_transport(&self) -> Result<DiagnosticFinding, Self::Error>;
}

pub trait ServiceContextProbePort {
    type Error;

    fn probe_service_context(&self) -> Result<DiagnosticFinding, Self::Error>;
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
}
