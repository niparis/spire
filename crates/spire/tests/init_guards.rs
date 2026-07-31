//! `spire init` checks terminal safety before opening its full-screen editor.
//!
//! The editor itself is covered by terminal-free application and TestBackend
//! tests; this file covers the process-level guard and its no-write behavior.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn sandbox(name: &str) -> PathBuf {
    // Canonicalize first: Spire rejects paths that traverse a symlink, and the
    // platform temp directory is one on macOS.
    let root = env::temp_dir()
        .canonicalize()
        .expect("the temp directory should resolve")
        .join(format!("spire-init-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("sandbox root should be creatable");
    root
}

fn init(root: &Path) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_spire"))
        .arg("init")
        .env("RUST_LOG", "")
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .stdin(std::process::Stdio::null())
        .output()
        .expect("Spire binary should run");

    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), combined)
}

#[test]
fn init_refuses_to_run_without_a_terminal() {
    let root = sandbox("no-tty");
    let (success, output) = init(&root);

    assert!(!success, "a non-interactive init must fail:\n{output}");
    assert!(output.contains("requires a terminal"), "{output}");
    assert!(
        !root.join("config/spire/config.yaml").exists(),
        "a refused init must not write a configuration"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn init_keeps_an_existing_configuration_untouched_when_no_terminal_is_available() {
    let root = sandbox("existing");
    let config = root.join("config/spire/config.yaml");
    fs::create_dir_all(config.parent().expect("parent")).expect("config root should be creatable");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/config/valid-dispatch.yaml");
    let original =
        fs::read_to_string(fixture).expect("the valid config fixture should be readable");
    fs::write(&config, &original).expect("the existing config should be writable");

    let (success, output) = init(&root);

    assert!(
        !success,
        "init must fail before entering the editor without a terminal:\n{output}"
    );
    assert!(output.contains("requires a terminal"), "{output}");
    assert_eq!(
        fs::read_to_string(&config).expect("the existing config should still be readable"),
        original,
        "the existing configuration must be untouched"
    );
    let _ = fs::remove_dir_all(&root);
}
