use std::process::Command;

fn run_version(flag: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_spire"))
        .arg(flag)
        .output()
        .expect("Spire binary should run");

    assert!(
        output.status.success(),
        "{flag} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("version output should be UTF-8")
        .trim()
        .to_owned()
}

#[test]
fn version_flags_report_the_cargo_package_version() {
    let expected = format!("spire {}", env!("CARGO_PKG_VERSION"));

    assert_eq!(run_version("--version"), expected);
    assert_eq!(run_version("-V"), expected);
}
