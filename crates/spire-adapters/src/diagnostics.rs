//! Bounded, non-mutating process diagnostics for harnesses, Git/SSH, and systemd.

use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;
use spire_application::{
    AuthenticationState, GitTransportProbe, GitTransportProbePort, HarnessProbe, HarnessProbePort,
    ProbeConfidence, ServiceContextProbe, ServiceContextProbePort,
};
use thiserror::Error;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_OUTPUT_LIMIT: usize = 64 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DiagnosticAdapterError {
    #[error("diagnostic executable is unavailable")]
    ExecutableUnavailable,
    #[error("diagnostic command timed out")]
    Timeout,
    #[error("diagnostic output exceeded its limit")]
    OutputTooLarge,
    #[error("diagnostic output was malformed or undocumented")]
    MalformedOutput,
    #[error("repository path is invalid")]
    InvalidRepository,
}

#[derive(Debug, Clone)]
pub struct CommandRequest {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub current_dir: Option<PathBuf>,
    pub timeout: Duration,
    pub output_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub trait CommandExecutor {
    fn execute(&self, request: &CommandRequest) -> Result<CommandOutput, DiagnosticAdapterError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemCommandExecutor;

impl CommandExecutor for SystemCommandExecutor {
    fn execute(&self, request: &CommandRequest) -> Result<CommandOutput, DiagnosticAdapterError> {
        let mut command = Command::new(&request.executable);
        command
            .args(&request.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(current_dir) = &request.current_dir {
            command.current_dir(current_dir);
        }
        let mut child = command
            .spawn()
            .map_err(|_| DiagnosticAdapterError::ExecutableUnavailable)?;
        let stdout = child.stdout.take().expect("piped stdout is available");
        let stderr = child.stderr.take().expect("piped stderr is available");
        let read_limit = request.output_limit.saturating_add(1) as u64;
        let stdout_reader = thread::spawn(move || read_bounded(stdout, read_limit));
        let stderr_reader = thread::spawn(move || read_bounded(stderr, read_limit));
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|_| DiagnosticAdapterError::MalformedOutput)?
            {
                break status;
            }
            if started.elapsed() >= request.timeout {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(DiagnosticAdapterError::Timeout);
            }
            thread::sleep(Duration::from_millis(10));
        };
        let stdout = stdout_reader
            .join()
            .map_err(|_| DiagnosticAdapterError::MalformedOutput)??;
        let stderr = stderr_reader
            .join()
            .map_err(|_| DiagnosticAdapterError::MalformedOutput)??;
        if stdout.len() > request.output_limit || stderr.len() > request.output_limit {
            return Err(DiagnosticAdapterError::OutputTooLarge);
        }
        Ok(CommandOutput {
            success: status.success(),
            stdout: String::from_utf8(stdout)
                .map_err(|_| DiagnosticAdapterError::MalformedOutput)?,
            stderr: String::from_utf8(stderr)
                .map_err(|_| DiagnosticAdapterError::MalformedOutput)?,
        })
    }
}

fn read_bounded(reader: impl Read, limit: u64) -> Result<Vec<u8>, DiagnosticAdapterError> {
    let mut bytes = Vec::new();
    reader
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|_| DiagnosticAdapterError::MalformedOutput)?;
    Ok(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessKind {
    Codex,
    ClaudeCode,
}

#[derive(Debug, Clone)]
pub struct HarnessProbeSpec {
    pub kind: HarnessKind,
    pub executable: PathBuf,
    pub configured_models: Vec<String>,
    pub configured_efforts: Vec<String>,
}

pub struct ProcessHarnessProbe<E> {
    executor: E,
    spec: HarnessProbeSpec,
}

impl<E> ProcessHarnessProbe<E> {
    pub fn new(executor: E, spec: HarnessProbeSpec) -> Self {
        Self { executor, spec }
    }

    fn request(&self, args: &[&str]) -> CommandRequest {
        CommandRequest {
            executable: self.spec.executable.clone(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            current_dir: None,
            timeout: DEFAULT_TIMEOUT,
            output_limit: DEFAULT_OUTPUT_LIMIT,
        }
    }
}

impl<E: CommandExecutor> HarnessProbePort for ProcessHarnessProbe<E> {
    type Error = DiagnosticAdapterError;

    fn probe_harness(&self, harness: &str) -> Result<HarnessProbe, Self::Error> {
        let version = self.executor.execute(&self.request(&["--version"]))?;
        let version = version
            .success
            .then(|| version.stdout.trim().to_owned())
            .filter(|value| !value.is_empty());
        let auth = match self.spec.kind {
            HarnessKind::Codex => self.executor.execute(&self.request(&["login", "status"]))?,
            HarnessKind::ClaudeCode => self
                .executor
                .execute(&self.request(&["auth", "status", "--json"]))?,
        };
        let mut state = match self.spec.kind {
            HarnessKind::Codex => normalize_codex_auth(&auth),
            HarnessKind::ClaudeCode => normalize_claude_auth(&auth),
        };
        if !version
            .as_deref()
            .is_some_and(|value| approved_version(self.spec.kind, value))
        {
            state = AuthenticationState::Ambiguous;
        }
        Ok(HarnessProbe {
            harness: harness.to_owned(),
            executable: self.spec.executable.display().to_string(),
            version,
            state,
            supported_models: self.spec.configured_models.clone(),
            supported_efforts: self.spec.configured_efforts.clone(),
            confidence: if state == AuthenticationState::Ambiguous {
                ProbeConfidence::Unknown
            } else {
                ProbeConfidence::Confirmed
            },
            remediation: (state != AuthenticationState::Authenticated)
                .then(|| format!("authenticate {harness} as the configured runtime user")),
        })
    }
}

fn approved_version(kind: HarnessKind, value: &str) -> bool {
    match kind {
        HarnessKind::Codex => value.strip_prefix("codex-cli ").is_some_and(version_number),
        HarnessKind::ClaudeCode => value
            .strip_suffix(" (Claude Code)")
            .is_some_and(version_number),
    }
}

fn version_number(value: &str) -> bool {
    !value.is_empty()
        && value
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn normalize_codex_auth(output: &CommandOutput) -> AuthenticationState {
    let value = format!("{}\n{}", output.stdout, output.stderr);
    if output.success
        && (value.trim() == "Logged in using ChatGPT"
            || value.trim() == "Logged in using an API key")
    {
        AuthenticationState::Authenticated
    } else if value.contains("Not logged in") || value.contains("not logged in") {
        AuthenticationState::Unavailable
    } else {
        AuthenticationState::Ambiguous
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeAuthStatus {
    logged_in: bool,
}

fn normalize_claude_auth(output: &CommandOutput) -> AuthenticationState {
    match serde_json::from_str::<ClaudeAuthStatus>(&output.stdout) {
        Ok(status) if output.success && status.logged_in => AuthenticationState::Authenticated,
        Ok(_) => AuthenticationState::Unavailable,
        Err(_) => AuthenticationState::Ambiguous,
    }
}

pub struct GitCliProbe<E> {
    executor: E,
    git_executable: PathBuf,
    repository_path: PathBuf,
}

impl<E> GitCliProbe<E> {
    pub fn new(
        executor: E,
        git_executable: impl Into<PathBuf>,
        repository_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            executor,
            git_executable: git_executable.into(),
            repository_path: repository_path.into(),
        }
    }

    fn request(&self, args: &[&str]) -> CommandRequest {
        CommandRequest {
            executable: self.git_executable.clone(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            current_dir: Some(self.repository_path.clone()),
            timeout: DEFAULT_TIMEOUT,
            output_limit: DEFAULT_OUTPUT_LIMIT,
        }
    }
}

impl<E: CommandExecutor> GitTransportProbePort for GitCliProbe<E> {
    type Error = DiagnosticAdapterError;

    fn probe_git_transport(&self) -> Result<GitTransportProbe, Self::Error> {
        if !self.repository_path.is_absolute() {
            return Err(DiagnosticAdapterError::InvalidRepository);
        }
        let inside = self
            .executor
            .execute(&self.request(&["rev-parse", "--is-inside-work-tree"]))?;
        if !inside.success || inside.stdout.trim() != "true" {
            return Err(DiagnosticAdapterError::InvalidRepository);
        }
        let remote = self
            .executor
            .execute(&self.request(&["remote", "get-url", "--all", "origin"]))?;
        let remote_urls = remote
            .stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();
        let (remote_url, canonical_repository) = match remote_urls.as_slice() {
            [remote_url] => (
                Some((*remote_url).to_owned()),
                normalize_github_remote(remote_url),
            ),
            _ => (None, None),
        };
        let default_branch = self
            .executor
            .execute(&self.request(&[
                "symbolic-ref",
                "--quiet",
                "--short",
                "refs/remotes/origin/HEAD",
            ]))
            .ok()
            .filter(|output| output.success)
            .and_then(|output| {
                output
                    .stdout
                    .trim()
                    .strip_prefix("origin/")
                    .map(str::to_owned)
            });
        let fetch = self.executor.execute(&self.request(&[
            "ls-remote",
            "--exit-code",
            "origin",
            "HEAD",
        ]))?;
        let fetch_state = if fetch.success {
            AuthenticationState::Authenticated
        } else {
            AuthenticationState::PermissionDenied
        };
        let ssh_agent = std::env::var_os("SSH_AUTH_SOCK").map(PathBuf::from);
        let ephemeral_agent_risk = ssh_agent.as_ref().is_some_and(|path| {
            !path.exists() || path.starts_with("/tmp") || path.starts_with("/var/folders")
        });
        Ok(GitTransportProbe {
            repository_path: self.repository_path.display().to_string(),
            remote_name: Some("origin".into()),
            remote_url,
            canonical_repository,
            default_branch,
            fetch_state,
            push_state: AuthenticationState::Unsupported,
            ephemeral_agent_risk,
            confidence: if fetch_state == AuthenticationState::Authenticated {
                ProbeConfidence::Confirmed
            } else {
                ProbeConfidence::Unknown
            },
            remediation: (fetch_state != AuthenticationState::Authenticated).then(|| {
                "verify the runtime user's SSH configuration and run git ls-remote origin HEAD"
                    .into()
            }),
        })
    }
}

pub fn normalize_github_remote(remote: &str) -> Option<String> {
    let value = remote.trim();
    let path = value
        .strip_prefix("git@github.com:")
        .or_else(|| value.strip_prefix("ssh://git@github.com/"))
        .or_else(|| value.strip_prefix("https://github.com/"))?;
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let repository = parts.next()?;
    if parts.next().is_some() || !safe_git_name(owner) || !safe_git_name(repository) {
        return None;
    }
    Some(format!("{owner}/{repository}"))
}

fn safe_git_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

pub struct SystemdServiceContextProbe<E> {
    executor: E,
    systemctl: PathBuf,
    loginctl: PathBuf,
    runtime_user: String,
}

impl<E> SystemdServiceContextProbe<E> {
    pub fn new(executor: E, runtime_user: impl Into<String>) -> Self {
        Self {
            executor,
            systemctl: PathBuf::from("systemctl"),
            loginctl: PathBuf::from("loginctl"),
            runtime_user: runtime_user.into(),
        }
    }

    fn request(executable: PathBuf, args: &[&str]) -> CommandRequest {
        CommandRequest {
            executable,
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            current_dir: None,
            timeout: DEFAULT_TIMEOUT,
            output_limit: DEFAULT_OUTPUT_LIMIT,
        }
    }
}

impl<E: CommandExecutor> ServiceContextProbePort for SystemdServiceContextProbe<E> {
    type Error = DiagnosticAdapterError;

    fn probe_service_context(&self) -> Result<ServiceContextProbe, Self::Error> {
        let installed = self.executor.execute(&Self::request(
            self.systemctl.clone(),
            &[
                "--user",
                "show",
                "spire.service",
                "--property",
                "LoadState",
                "--value",
            ],
        ))?;
        let active = self.executor.execute(&Self::request(
            self.systemctl.clone(),
            &["--user", "is-active", "spire.service"],
        ))?;
        let linger = self.executor.execute(&Self::request(
            self.loginctl.clone(),
            &["show-user", &self.runtime_user, "-p", "Linger", "--value"],
        ))?;
        let unit_installed = installed.success && installed.stdout.trim() == "loaded";
        let unit_active = active.success && active.stdout.trim() == "active";
        let lingering_enabled = linger.success && linger.stdout.trim() == "yes";
        let ssh_agent_available = std::env::var_os("SSH_AUTH_SOCK")
            .map(PathBuf::from)
            .is_some_and(|path| path.exists());
        let state = if unit_installed && unit_active && lingering_enabled {
            AuthenticationState::Authenticated
        } else {
            AuthenticationState::Unavailable
        };
        Ok(ServiceContextProbe {
            unit_installed,
            unit_active,
            lingering_enabled,
            runtime_user: self.runtime_user.clone(),
            ssh_agent_available,
            state,
            remediation: (state != AuthenticationState::Authenticated).then(|| {
                "install/start the user service and enable lingering for the runtime user".into()
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::Mutex};

    struct FakeExecutor {
        outputs: Mutex<VecDeque<Result<CommandOutput, DiagnosticAdapterError>>>,
    }

    impl FakeExecutor {
        fn new(outputs: Vec<CommandOutput>) -> Self {
            Self {
                outputs: Mutex::new(outputs.into_iter().map(Ok).collect()),
            }
        }
    }

    impl CommandExecutor for FakeExecutor {
        fn execute(
            &self,
            _request: &CommandRequest,
        ) -> Result<CommandOutput, DiagnosticAdapterError> {
            self.outputs.lock().unwrap().pop_front().unwrap()
        }
    }

    fn output(success: bool, stdout: &str) -> CommandOutput {
        CommandOutput {
            success,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    #[test]
    fn captured_harness_statuses_fail_closed_on_unknown_output() {
        const CODEX_AUTHENTICATED: &str =
            include_str!("../../../tests/fixtures/auth/codex-authenticated.txt");
        const CLAUDE_AUTHENTICATED: &str =
            include_str!("../../../tests/fixtures/auth/claude-authenticated.json");
        let codex = ProcessHarnessProbe::new(
            FakeExecutor::new(vec![
                output(true, "codex-cli 0.145.0\n"),
                output(true, CODEX_AUTHENTICATED),
            ]),
            HarnessProbeSpec {
                kind: HarnessKind::Codex,
                executable: "/opt/codex".into(),
                configured_models: vec!["model".into()],
                configured_efforts: vec!["high".into()],
            },
        );
        assert_eq!(
            codex.probe_harness("codex").unwrap().state,
            AuthenticationState::Authenticated
        );

        let claude = ProcessHarnessProbe::new(
            FakeExecutor::new(vec![
                output(true, "2.1.148 (Claude Code)\n"),
                output(true, CLAUDE_AUTHENTICATED),
            ]),
            HarnessProbeSpec {
                kind: HarnessKind::ClaudeCode,
                executable: "/opt/claude".into(),
                configured_models: vec!["model".into()],
                configured_efforts: vec!["high".into()],
            },
        );
        let probe = claude.probe_harness("claude-code").unwrap();
        assert_eq!(probe.state, AuthenticationState::Authenticated);
        assert!(
            !serde_json::to_string(&probe)
                .unwrap()
                .contains("redacted@example")
        );

        assert_eq!(
            normalize_codex_auth(&output(true, "future undocumented output")),
            AuthenticationState::Ambiguous
        );

        let unknown_version = ProcessHarnessProbe::new(
            FakeExecutor::new(vec![
                output(true, "future-version"),
                output(true, CODEX_AUTHENTICATED),
            ]),
            HarnessProbeSpec {
                kind: HarnessKind::Codex,
                executable: "/opt/codex".into(),
                configured_models: vec![],
                configured_efforts: vec![],
            },
        );
        assert_eq!(
            unknown_version.probe_harness("codex").unwrap().state,
            AuthenticationState::Ambiguous
        );
    }

    #[test]
    fn logged_out_and_malformed_statuses_are_not_authenticated() {
        assert_eq!(
            normalize_codex_auth(&output(
                false,
                include_str!("../../../tests/fixtures/auth/codex-logged-out.txt")
            )),
            AuthenticationState::Unavailable
        );
        assert_eq!(
            normalize_claude_auth(&output(
                false,
                include_str!("../../../tests/fixtures/auth/claude-logged-out.json")
            )),
            AuthenticationState::Unavailable
        );
        assert_eq!(
            normalize_claude_auth(&output(true, "{future-json")),
            AuthenticationState::Ambiguous
        );
    }

    #[test]
    fn system_executor_enforces_timeout_and_output_bounds() {
        let executor = SystemCommandExecutor;
        assert_eq!(
            executor.execute(&CommandRequest {
                executable: "/bin/sleep".into(),
                args: vec!["1".into()],
                current_dir: None,
                timeout: Duration::from_millis(10),
                output_limit: 128,
            }),
            Err(DiagnosticAdapterError::Timeout)
        );
        assert_eq!(
            executor.execute(&CommandRequest {
                executable: "/usr/bin/yes".into(),
                args: vec![],
                current_dir: None,
                timeout: Duration::from_secs(1),
                output_limit: 128,
            }),
            Err(DiagnosticAdapterError::OutputTooLarge)
        );
    }

    #[test]
    fn github_remote_normalization_is_allowlisted() {
        for remote in [
            "git@github.com:owner/repository.git",
            "ssh://git@github.com/owner/repository.git",
            "https://github.com/owner/repository",
        ] {
            assert_eq!(
                normalize_github_remote(remote).as_deref(),
                Some("owner/repository")
            );
        }
        assert_eq!(
            normalize_github_remote("git@example.test:owner/repository.git"),
            None
        );
        assert_eq!(
            normalize_github_remote("git@github.com:owner/repo/extra"),
            None
        );
    }

    #[test]
    fn git_probe_never_infers_push_from_fetch() {
        let probe = GitCliProbe::new(
            FakeExecutor::new(vec![
                output(true, "true\n"),
                output(true, "git@github.com:owner/repository.git\n"),
                output(true, "origin/main\n"),
                output(true, "abc\tHEAD\n"),
            ]),
            "git",
            "/tmp/repository",
        )
        .probe_git_transport()
        .unwrap();
        assert_eq!(probe.fetch_state, AuthenticationState::Authenticated);
        assert_eq!(probe.push_state, AuthenticationState::Unsupported);
        assert_eq!(
            probe.canonical_repository.as_deref(),
            Some("owner/repository")
        );
    }

    #[test]
    fn service_context_requires_installation_activity_and_lingering() {
        let probe = SystemdServiceContextProbe::new(
            FakeExecutor::new(vec![
                output(true, "loaded\n"),
                output(true, "active\n"),
                output(true, "yes\n"),
            ]),
            "operator",
        )
        .probe_service_context()
        .unwrap();
        assert_eq!(probe.state, AuthenticationState::Authenticated);

        let blocked = SystemdServiceContextProbe::new(
            FakeExecutor::new(vec![
                output(true, "loaded\n"),
                output(false, "inactive\n"),
                output(true, "no\n"),
            ]),
            "operator",
        )
        .probe_service_context()
        .unwrap();
        assert_eq!(blocked.state, AuthenticationState::Unavailable);
    }
}
