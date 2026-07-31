//! `spire init` refuses to run where it could destroy existing state.
//!
//! The interview itself needs a Linear workspace and a terminal, so only the
//! guards that run before any prompt are covered here.

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
fn init_refuses_to_replace_an_existing_configuration() {
    let root = sandbox("existing");
    let config = root.join("config/spire/config.yaml");
    fs::create_dir_all(config.parent().expect("parent")).expect("config root should be creatable");
    fs::write(&config, "schema_version: 4\n").expect("the existing config should be writable");

    let (success, output) = init(&root);

    assert!(
        !success,
        "init must not overwrite a configuration:\n{output}"
    );
    assert!(output.contains("already exists"), "{output}");
    assert_eq!(
        fs::read_to_string(&config).expect("the existing config should still be readable"),
        "schema_version: 4\n",
        "the existing configuration must be untouched"
    );
    let _ = fs::remove_dir_all(&root);
}
