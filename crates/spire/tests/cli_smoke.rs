//! Behavioural smoke coverage for the approved user-facing commands.
//!
//! Only hermetic commands are exercised here. `start`, `stop`, `status`,
//! `service install`, `auth`, `doctor`, `projects`, and `serve` reach
//! systemd, the secret store, the database, or a listening socket, so they
//! are covered by their own integration suites rather than smoke tests.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU32, Ordering},
};

/// Commands whose `--help` must render. Mirrors the approved surface in
/// `docs/decisions/cli-command-surface.md`.
const VISIBLE_TOP_LEVEL: &[&str] = &[
    "init", "paths", "service", "start", "stop", "status", "config", "auth", "doctor", "projects",
    "serve",
];

fn fixture_config() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/config/valid-dispatch.yaml")
        .canonicalize()
        .expect("the checked-in configuration fixture should exist")
}

/// An isolated set of XDG roots so path resolution never reads or writes the
/// developer's real Spire installation.
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);

        // Canonicalize before use: Spire rejects paths that traverse a
        // symlink, and the platform temp directory is one on macOS.
        let base = env::temp_dir()
            .canonicalize()
            .expect("the temp directory should resolve");
        let root = base.join(format!(
            "spire-cli-smoke-{}-{name}-{unique}",
            std::process::id()
        ));

        fs::create_dir_all(&root).expect("sandbox root should be creatable");
        Self { root }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_spire"));
        command
            .env("RUST_LOG", "")
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_CACHE_HOME", self.root.join("cache"));
        command
    }

    /// Runs against the checked-in valid fixture.
    fn run(&self, args: &[&str]) -> Output {
        self.run_with_config(&fixture_config(), args)
    }

    fn run_with_config(&self, config: &Path, args: &[&str]) -> Output {
        let output = self
            .command()
            .args(args)
            .arg("--config")
            .arg(config)
            .output()
            .expect("Spire binary should run");

        Output {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Output {
    success: bool,
    stdout: String,
    stderr: String,
}

impl Output {
    fn expect_success(self, context: &str) -> String {
        assert!(
            self.success,
            "{context} should succeed\nstdout:\n{}\nstderr:\n{}",
            self.stdout, self.stderr
        );
        self.stdout
    }

    fn expect_failure(self, context: &str) -> String {
        assert!(
            !self.success,
            "{context} should fail\nstdout:\n{}\nstderr:\n{}",
            self.stdout, self.stderr
        );
        self.stderr
    }
}

#[test]
fn config_validate_accepts_the_checked_in_fixture() {
    let sandbox = Sandbox::new("validate");
    let stdout = sandbox
        .run(&["config", "validate"])
        .expect_success("`config validate` against the valid fixture");

    assert!(
        stdout.contains("configuration is valid"),
        "unexpected output: {stdout}"
    );
}

#[test]
fn config_validate_rejects_a_malformed_configuration() {
    let sandbox = Sandbox::new("validate-bad");
    let broken = sandbox.root.join("broken.yaml");
    fs::write(&broken, "schema_version: 4\nlinear: not-a-mapping\n")
        .expect("the malformed fixture should be writable");

    let stderr = sandbox
        .run_with_config(&broken, &["config", "validate"])
        .expect_failure("`config validate` against a malformed configuration");

    assert!(
        stderr.contains("failed to load configuration"),
        "expected a load failure, got: {stderr}"
    );
}

#[test]
fn config_validate_rejects_a_superseded_schema_version() {
    let sandbox = Sandbox::new("validate-schema");
    let superseded = sandbox.root.join("schema3.yaml");
    let downgraded = fs::read_to_string(fixture_config())
        .expect("the fixture should be readable")
        .replace("schema_version: 4", "schema_version: 3");
    fs::write(&superseded, downgraded).expect("the downgraded fixture should be writable");

    sandbox
        .run_with_config(&superseded, &["config", "validate"])
        .expect_failure("`config validate` against a schema 3 configuration");
}

#[test]
fn config_show_emits_the_configuration() {
    let sandbox = Sandbox::new("show");
    let stdout = sandbox
        .run(&["config", "show"])
        .expect_success("`config show`");

    assert!(
        stdout.contains("schema_version: 4"),
        "unexpected output: {stdout}"
    );
}

#[test]
fn config_show_effective_reports_the_resolved_path() {
    let sandbox = Sandbox::new("show-effective");
    let stdout = sandbox
        .run(&["config", "show", "--effective"])
        .expect_success("`config show --effective`");

    assert!(
        stdout.contains("configuration_path:"),
        "expected the resolved path, got: {stdout}"
    );
}

#[test]
fn config_path_reports_the_override() {
    let sandbox = Sandbox::new("path");
    let stdout = sandbox
        .run(&["config", "path"])
        .expect_success("`config path`");
    let fixture = fixture_config();

    assert!(
        stdout.contains(&fixture.display().to_string()),
        "expected the overridden config path, got: {stdout}"
    );
}

#[test]
fn paths_reports_the_sandboxed_roots() {
    let sandbox = Sandbox::new("paths-text");
    let stdout = sandbox.run(&["paths"]).expect_success("`paths`");

    for root in ["data:", "state:", "cache:"] {
        assert!(stdout.contains(root), "missing `{root}` in: {stdout}");
    }
    assert!(
        stdout.contains(&sandbox.root.display().to_string()),
        "paths escaped the sandbox: {stdout}"
    );
}

#[test]
fn paths_json_is_machine_readable() {
    let sandbox = Sandbox::new("paths-json");
    let stdout = sandbox
        .run(&["paths", "--format", "json"])
        .expect_success("`paths --format json`");

    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("`--format json` must emit parseable JSON");

    for key in [
        "profile",
        "config_file",
        "config_root",
        "data_root",
        "state_root",
        "cache_root",
    ] {
        assert!(
            parsed.get(key).is_some(),
            "missing `{key}` in the JSON payload: {stdout}"
        );
    }
}

#[test]
fn every_visible_command_renders_help() {
    let sandbox = Sandbox::new("help");
    for command in VISIBLE_TOP_LEVEL {
        let output = sandbox
            .command()
            .args([command, "--help"])
            .output()
            .expect("Spire binary should run");

        assert!(
            output.status.success(),
            "`spire {command} --help` should render:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
