//! GitHub App registration, installation authentication, and permission diagnosis.
//!
//! Durable private keys remain in the secret-store adapter. Installation tokens
//! are minted here, cached only in memory, and never exposed through application
//! DTOs.

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT},
};
use serde::{Deserialize, Serialize};
use spire_application::{
    AuthenticationState, ClockPort, ProbeConfidence, ServiceAuthenticationProbe,
    ServiceAuthenticationProbePort,
};
use thiserror::Error;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::sync::Mutex;

const GITHUB_API: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";
const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl ClockPort for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

#[derive(Debug, Error)]
pub enum GitHubAppError {
    #[error("GitHub App private key is invalid")]
    InvalidPrivateKey,
    #[error("GitHub App response was malformed")]
    MalformedResponse,
    #[error("GitHub App authentication was denied")]
    Authentication,
    #[error("GitHub App permission was denied")]
    PermissionDenied,
    #[error("GitHub App installation is unavailable")]
    Unavailable,
    #[error("GitHub App request was rate limited")]
    RateLimited,
    #[error("GitHub App transport failed")]
    Transport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GitHubAppManifest {
    pub name: String,
    pub url: String,
    pub redirect_url: String,
    pub hook_attributes: ManifestHook,
    pub public: bool,
    pub default_permissions: BTreeMap<String, String>,
    pub default_events: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestHook {
    pub url: String,
    pub active: bool,
}

impl GitHubAppManifest {
    pub fn spire(
        name: impl Into<String>,
        homepage_url: impl Into<String>,
        redirect_url: impl Into<String>,
        webhook_url: impl Into<String>,
        contents_write: bool,
    ) -> Self {
        Self {
            name: name.into(),
            url: homepage_url.into(),
            redirect_url: redirect_url.into(),
            hook_attributes: ManifestHook {
                url: webhook_url.into(),
                active: true,
            },
            public: false,
            default_permissions: BTreeMap::from([
                ("actions".into(), "read".into()),
                ("checks".into(), "read".into()),
                (
                    "contents".into(),
                    if contents_write { "write" } else { "read" }.into(),
                ),
                ("metadata".into(), "read".into()),
                ("pull_requests".into(), "write".into()),
            ]),
            default_events: vec![
                "check_run".into(),
                "check_suite".into(),
                "pull_request".into(),
                "push".into(),
                "workflow_run".into(),
            ],
        }
    }
}

pub struct ManifestConversion {
    pub app_id: u64,
    pub app_slug: String,
    pub client_id: String,
    pub private_key_pem: String,
    pub webhook_secret: String,
    pub html_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubAppMetadata {
    pub app_id: u64,
    pub app_slug: String,
    pub client_id: String,
    pub html_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InstallationRecord {
    pub id: u64,
    pub app_id: u64,
    pub target_type: String,
    pub permissions: BTreeMap<String, String>,
    pub events: Vec<String>,
    pub suspended_at: Option<String>,
}

pub struct InstallationToken {
    token: String,
    expires_at: SystemTime,
}

impl InstallationToken {
    pub fn expose_to_github_adapter(&self) -> &str {
        &self.token
    }
}

#[allow(async_fn_in_trait)]
pub trait GitHubAppApi {
    async fn exchange_manifest_code(
        &self,
        code: &str,
    ) -> Result<ManifestConversion, GitHubAppError>;

    async fn get_installation(
        &self,
        app_jwt: &str,
        installation_id: u64,
    ) -> Result<InstallationRecord, GitHubAppError>;

    async fn create_installation_token(
        &self,
        app_jwt: &str,
        installation_id: u64,
        permissions: &BTreeMap<String, String>,
    ) -> Result<InstallationToken, GitHubAppError>;
}

#[derive(Clone)]
pub struct GitHubAppHttpApi {
    client: Client,
    api_base: String,
}

impl GitHubAppHttpApi {
    pub fn new(timeout: Duration) -> Result<Self, GitHubAppError> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("spire-orchestrator"));
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "x-github-api-version",
            HeaderValue::from_static(API_VERSION),
        );
        Ok(Self {
            client: Client::builder()
                .default_headers(headers)
                .timeout(timeout)
                .build()
                .map_err(|_| GitHubAppError::Transport)?,
            api_base: GITHUB_API.into(),
        })
    }

    #[cfg(test)]
    fn with_api_base(timeout: Duration, api_base: String) -> Result<Self, GitHubAppError> {
        let mut api = Self::new(timeout)?;
        api.api_base = api_base;
        Ok(api)
    }
}

#[derive(Deserialize)]
struct RawManifestConversion {
    id: u64,
    slug: String,
    client_id: String,
    pem: String,
    webhook_secret: String,
    html_url: String,
}

#[derive(Deserialize)]
struct RawInstallationToken {
    token: String,
    expires_at: String,
}

#[allow(async_fn_in_trait)]
impl GitHubAppApi for GitHubAppHttpApi {
    async fn exchange_manifest_code(
        &self,
        code: &str,
    ) -> Result<ManifestConversion, GitHubAppError> {
        if code.is_empty() || code.len() > 256 {
            return Err(GitHubAppError::MalformedResponse);
        }
        let response = self
            .client
            .post(format!(
                "{}/app-manifests/{}/conversions",
                self.api_base,
                urlencoding::encode(code)
            ))
            .send()
            .await
            .map_err(|_| GitHubAppError::Transport)?;
        let raw: RawManifestConversion = decode_response(response).await?;
        if raw.pem.is_empty() || raw.webhook_secret.is_empty() {
            return Err(GitHubAppError::MalformedResponse);
        }
        Ok(ManifestConversion {
            app_id: raw.id,
            app_slug: raw.slug,
            client_id: raw.client_id,
            private_key_pem: raw.pem,
            webhook_secret: raw.webhook_secret,
            html_url: raw.html_url,
        })
    }

    async fn get_installation(
        &self,
        app_jwt: &str,
        installation_id: u64,
    ) -> Result<InstallationRecord, GitHubAppError> {
        let response = self
            .client
            .get(format!(
                "{}/app/installations/{installation_id}",
                self.api_base
            ))
            .header(AUTHORIZATION, format!("Bearer {app_jwt}"))
            .send()
            .await
            .map_err(|_| GitHubAppError::Transport)?;
        decode_response(response).await
    }

    async fn create_installation_token(
        &self,
        app_jwt: &str,
        installation_id: u64,
        permissions: &BTreeMap<String, String>,
    ) -> Result<InstallationToken, GitHubAppError> {
        let response = self
            .client
            .post(format!(
                "{}/app/installations/{installation_id}/access_tokens",
                self.api_base
            ))
            .header(AUTHORIZATION, format!("Bearer {app_jwt}"))
            .json(&serde_json::json!({ "permissions": permissions }))
            .send()
            .await
            .map_err(|_| GitHubAppError::Transport)?;
        let raw: RawInstallationToken = decode_response(response).await?;
        let expires_at = OffsetDateTime::parse(&raw.expires_at, &Rfc3339)
            .map_err(|_| GitHubAppError::MalformedResponse)?;
        if raw.token.is_empty() {
            return Err(GitHubAppError::MalformedResponse);
        }
        Ok(InstallationToken {
            token: raw.token,
            expires_at: UNIX_EPOCH
                + Duration::from_secs(
                    expires_at
                        .unix_timestamp()
                        .try_into()
                        .map_err(|_| GitHubAppError::MalformedResponse)?,
                ),
        })
    }
}

async fn decode_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, GitHubAppError> {
    match response.status() {
        StatusCode::UNAUTHORIZED => return Err(GitHubAppError::Authentication),
        StatusCode::FORBIDDEN => return Err(GitHubAppError::PermissionDenied),
        StatusCode::NOT_FOUND => return Err(GitHubAppError::Unavailable),
        StatusCode::TOO_MANY_REQUESTS => return Err(GitHubAppError::RateLimited),
        status if !status.is_success() => return Err(GitHubAppError::Transport),
        _ => {}
    }
    response
        .json()
        .await
        .map_err(|_| GitHubAppError::MalformedResponse)
}

#[derive(Serialize)]
struct AppJwtClaims {
    iat: u64,
    exp: u64,
    iss: String,
}

struct CachedToken {
    token: String,
    expires_at: SystemTime,
}

pub struct GitHubAppTokenProvider<A, C> {
    api: A,
    clock: C,
    app_id: u64,
    installation_id: u64,
    private_key_pem: String,
    permissions: BTreeMap<String, String>,
    cache: Arc<Mutex<Option<CachedToken>>>,
}

impl<A, C> GitHubAppTokenProvider<A, C> {
    pub fn new(
        api: A,
        clock: C,
        app_id: u64,
        installation_id: u64,
        private_key_pem: String,
        permissions: BTreeMap<String, String>,
    ) -> Result<Self, GitHubAppError> {
        EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
            .map_err(|_| GitHubAppError::InvalidPrivateKey)?;
        Ok(Self {
            api,
            clock,
            app_id,
            installation_id,
            private_key_pem,
            permissions,
            cache: Arc::new(Mutex::new(None)),
        })
    }
}

impl<A: GitHubAppApi, C: ClockPort> GitHubAppTokenProvider<A, C> {
    fn app_jwt(&self) -> Result<String, GitHubAppError> {
        let now = self
            .clock
            .now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| GitHubAppError::Authentication)?
            .as_secs();
        let claims = AppJwtClaims {
            iat: now.saturating_sub(60),
            exp: now.saturating_add(9 * 60),
            iss: self.app_id.to_string(),
        };
        jsonwebtoken::encode(
            &Header::new(Algorithm::RS256),
            &claims,
            &EncodingKey::from_rsa_pem(self.private_key_pem.as_bytes())
                .map_err(|_| GitHubAppError::InvalidPrivateKey)?,
        )
        .map_err(|_| GitHubAppError::Authentication)
    }

    pub async fn installation_token(&self) -> Result<InstallationToken, GitHubAppError> {
        let mut cache = self.cache.lock().await;
        let refresh_at = self
            .clock
            .now()
            .checked_add(TOKEN_REFRESH_SKEW)
            .ok_or(GitHubAppError::Authentication)?;
        if let Some(cached) = cache.as_ref()
            && cached.expires_at > refresh_at
        {
            return Ok(InstallationToken {
                token: cached.token.clone(),
                expires_at: cached.expires_at,
            });
        }
        let token = self
            .api
            .create_installation_token(&self.app_jwt()?, self.installation_id, &self.permissions)
            .await?;
        *cache = Some(CachedToken {
            token: token.token.clone(),
            expires_at: token.expires_at,
        });
        Ok(token)
    }

    pub async fn installation(&self) -> Result<InstallationRecord, GitHubAppError> {
        self.api
            .get_installation(&self.app_jwt()?, self.installation_id)
            .await
    }
}

pub struct GitHubAppServiceProbe<A, C> {
    provider: GitHubAppTokenProvider<A, C>,
    required_permissions: BTreeMap<String, String>,
}

impl<A, C> GitHubAppServiceProbe<A, C> {
    pub fn new(
        provider: GitHubAppTokenProvider<A, C>,
        required_permissions: BTreeMap<String, String>,
    ) -> Self {
        Self {
            provider,
            required_permissions,
        }
    }
}

#[allow(async_fn_in_trait)]
impl<A: GitHubAppApi, C: ClockPort> ServiceAuthenticationProbePort for GitHubAppServiceProbe<A, C> {
    type Error = GitHubAppError;

    async fn probe_service(
        &self,
        service: &str,
    ) -> Result<ServiceAuthenticationProbe, Self::Error> {
        let installation = self.provider.installation().await?;
        let missing_permissions = self
            .required_permissions
            .iter()
            .filter(|(name, level)| {
                installation
                    .permissions
                    .get(*name)
                    .is_none_or(|actual| permission_rank(actual) < permission_rank(level))
            })
            .map(|(name, level)| format!("{name}:{level}"))
            .collect::<Vec<_>>();
        let unsafe_admin = installation
            .permissions
            .get("administration")
            .is_some_and(|level| permission_rank(level) >= permission_rank("write"));
        let state = if installation.suspended_at.is_some()
            || !missing_permissions.is_empty()
            || unsafe_admin
        {
            AuthenticationState::PermissionDenied
        } else {
            AuthenticationState::Authenticated
        };
        Ok(ServiceAuthenticationProbe {
            service: service.into(),
            state,
            identity: Some(format!(
                "app:{} installation:{} target:{}",
                installation.app_id, installation.id, installation.target_type
            )),
            expires_at: None,
            permissions: installation
                .permissions
                .iter()
                .map(|(name, level)| format!("{name}:{level}"))
                .collect(),
            missing_permissions,
            confidence: ProbeConfidence::Confirmed,
            remediation: (state != AuthenticationState::Authenticated).then(|| {
                "restore the approved GitHub App permissions and remove unsafe administration authority"
                    .into()
            }),
        })
    }
}

fn permission_rank(value: &str) -> u8 {
    match value {
        "read" => 1,
        "write" => 2,
        "admin" => 3,
        _ => 0,
    }
}

pub fn approved_installation_permissions(contents_write: bool) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("actions".into(), "read".into()),
        ("checks".into(), "read".into()),
        (
            "contents".into(),
            if contents_write { "write" } else { "read" }.into(),
        ),
        ("metadata".into(), "read".into()),
        ("pull_requests".into(), "write".into()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    };

    const TEST_RSA_KEY: &str = include_str!("../../../tests/fixtures/auth/github-app-test-key.pem");

    struct FixedClock(SystemTime);

    impl ClockPort for FixedClock {
        fn now(&self) -> SystemTime {
            self.0
        }
    }

    struct FakeApi {
        token_calls: AtomicUsize,
        installation: InstallationRecord,
        token: StdMutex<Option<InstallationToken>>,
    }

    #[allow(async_fn_in_trait)]
    impl GitHubAppApi for FakeApi {
        async fn exchange_manifest_code(
            &self,
            _code: &str,
        ) -> Result<ManifestConversion, GitHubAppError> {
            unreachable!()
        }

        async fn get_installation(
            &self,
            _app_jwt: &str,
            _installation_id: u64,
        ) -> Result<InstallationRecord, GitHubAppError> {
            Ok(self.installation.clone())
        }

        async fn create_installation_token(
            &self,
            _app_jwt: &str,
            _installation_id: u64,
            _permissions: &BTreeMap<String, String>,
        ) -> Result<InstallationToken, GitHubAppError> {
            self.token_calls.fetch_add(1, Ordering::SeqCst);
            self.token
                .lock()
                .unwrap()
                .take()
                .ok_or(GitHubAppError::Unavailable)
        }
    }

    fn fixture_api(now: SystemTime) -> FakeApi {
        FakeApi {
            token_calls: AtomicUsize::new(0),
            installation: InstallationRecord {
                id: 7,
                app_id: 42,
                target_type: "Organization".into(),
                permissions: approved_installation_permissions(false),
                events: vec!["pull_request".into()],
                suspended_at: None,
            },
            token: StdMutex::new(Some(InstallationToken {
                token: "installation-token".into(),
                expires_at: now + Duration::from_secs(3600),
            })),
        }
    }

    #[test]
    fn manifest_contains_only_approved_permissions_and_events() {
        let manifest = GitHubAppManifest::spire(
            "Spire",
            "https://spire.example.test",
            "http://127.0.0.1:1234/callback",
            "https://spire.example.test/webhooks/github",
            false,
        );
        assert_eq!(manifest.default_permissions["contents"], "read");
        assert_eq!(manifest.default_permissions["pull_requests"], "write");
        assert!(!manifest.default_permissions.contains_key("administration"));
        assert!(manifest.default_events.contains(&"workflow_run".to_owned()));
    }

    #[tokio::test]
    async fn token_refresh_is_cached_and_permission_probe_is_explicit() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let provider = GitHubAppTokenProvider::new(
            fixture_api(now),
            FixedClock(now),
            42,
            7,
            TEST_RSA_KEY.into(),
            approved_installation_permissions(false),
        )
        .unwrap();
        assert_eq!(
            provider
                .installation_token()
                .await
                .unwrap()
                .expose_to_github_adapter(),
            "installation-token"
        );
        assert_eq!(
            provider
                .installation_token()
                .await
                .unwrap()
                .expose_to_github_adapter(),
            "installation-token"
        );
        assert_eq!(provider.api.token_calls.load(Ordering::SeqCst), 1);

        let probe = GitHubAppServiceProbe::new(provider, approved_installation_permissions(false))
            .probe_service("github")
            .await
            .unwrap();
        assert_eq!(probe.state, AuthenticationState::Authenticated);
        assert!(probe.missing_permissions.is_empty());
    }

    #[tokio::test]
    async fn concurrent_refresh_mints_one_token() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let provider = Arc::new(
            GitHubAppTokenProvider::new(
                fixture_api(now),
                FixedClock(now),
                42,
                7,
                TEST_RSA_KEY.into(),
                approved_installation_permissions(false),
            )
            .unwrap(),
        );
        let first = {
            let provider = Arc::clone(&provider);
            tokio::spawn(async move {
                provider
                    .installation_token()
                    .await
                    .unwrap()
                    .expose_to_github_adapter()
                    .to_owned()
            })
        };
        let second = {
            let provider = Arc::clone(&provider);
            tokio::spawn(async move {
                provider
                    .installation_token()
                    .await
                    .unwrap()
                    .expose_to_github_adapter()
                    .to_owned()
            })
        };
        assert_eq!(first.await.unwrap(), "installation-token");
        assert_eq!(second.await.unwrap(), "installation-token");
        assert_eq!(provider.api.token_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn missing_or_unsafe_permissions_fail_closed() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let mut api = fixture_api(now);
        api.installation.permissions.remove("pull_requests");
        api.installation
            .permissions
            .insert("administration".into(), "write".into());
        let provider = GitHubAppTokenProvider::new(
            api,
            FixedClock(now),
            42,
            7,
            TEST_RSA_KEY.into(),
            approved_installation_permissions(false),
        )
        .unwrap();
        let probe = GitHubAppServiceProbe::new(provider, approved_installation_permissions(false))
            .probe_service("github")
            .await
            .unwrap();
        assert_eq!(probe.state, AuthenticationState::PermissionDenied);
        assert_eq!(probe.missing_permissions, vec!["pull_requests:write"]);
    }

    #[test]
    fn http_api_can_target_a_fixture_server_without_changing_production_base() {
        assert!(
            GitHubAppHttpApi::with_api_base(Duration::from_secs(1), "http://127.0.0.1".into())
                .is_ok()
        );
    }
}
