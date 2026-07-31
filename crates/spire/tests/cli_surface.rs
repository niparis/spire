//! Locks the approved CLI surface described in
//! `docs/decisions/cli-command-surface.md`. A failure here is a product
//! decision that needs the decision record updated in the same PR, not a
//! test to be adjusted until it passes.

use std::process::Command;

/// Commands `spire --help` is allowed to advertise.
const VISIBLE_TOP_LEVEL: &[&str] = &[
    "init", "paths", "service", "start", "stop", "status", "config", "auth", "doctor", "projects",
    "serve", "help",
];

/// Subcommands `spire projects --help` is allowed to advertise.
const VISIBLE_PROJECTS: &[&str] = &["list", "map", "show", "disable", "remove", "help"];

/// Hidden from help but still reachable. Unapproved and scheduled for
/// delete / merge / promote / move; until then existing callers keep working.
const HIDDEN_TOP_LEVEL: &[&str] = &[
    "dispatch",
    "db",
    "ops",
    "linear",
    "github",
    "scheduler",
    "runs",
];

/// Hidden `projects` subcommands, same contract as `HIDDEN_TOP_LEVEL`.
const HIDDEN_PROJECTS: &[&str] = &["doctor", "preflight", "reconcile"];

fn run(args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_spire"))
        .args(args)
        .env("RUST_LOG", "")
        .output()
        .expect("Spire binary should run");

    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.success(), combined)
}

/// Extracts the command names listed under the `Commands:` heading of a clap
/// help page.
fn advertised_commands(args: &[&str]) -> Vec<String> {
    let (success, help) = run(args);
    assert!(success, "`{}` should succeed:\n{help}", args.join(" "));

    let mut names = Vec::new();
    let mut inside = false;
    for line in help.lines() {
        if line.trim_end() == "Commands:" {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        // clap separates sections with a blank line.
        if line.trim().is_empty() {
            break;
        }
        let Some(name) = line.split_whitespace().next() else {
            continue;
        };
        names.push(name.to_owned());
    }

    assert!(
        !names.is_empty(),
        "`{}` produced no Commands: section:\n{help}",
        args.join(" ")
    );
    names.sort();
    names
}

fn sorted(values: &[&str]) -> Vec<String> {
    let mut owned: Vec<String> = values.iter().map(|value| (*value).to_owned()).collect();
    owned.sort();
    owned
}

#[test]
fn top_level_help_advertises_only_the_approved_surface() {
    assert_eq!(
        advertised_commands(&["--help"]),
        sorted(VISIBLE_TOP_LEVEL),
        "the top-level command surface changed; update \
         docs/decisions/cli-command-surface.md in the same PR"
    );
}

#[test]
fn projects_help_advertises_only_the_approved_surface() {
    assert_eq!(
        advertised_commands(&["projects", "--help"]),
        sorted(VISIBLE_PROJECTS),
        "the projects command surface changed; update \
         docs/decisions/cli-command-surface.md in the same PR"
    );
}

#[test]
fn hidden_commands_are_absent_from_top_level_help() {
    let advertised = advertised_commands(&["--help"]);
    for command in HIDDEN_TOP_LEVEL {
        assert!(
            !advertised.contains(&(*command).to_owned()),
            "`{command}` is hidden but appeared in `spire --help`"
        );
    }
}

#[test]
fn hidden_commands_are_absent_from_projects_help() {
    let advertised = advertised_commands(&["projects", "--help"]);
    for command in HIDDEN_PROJECTS {
        assert!(
            !advertised.contains(&(*command).to_owned()),
            "`projects {command}` is hidden but appeared in `spire projects --help`"
        );
    }
}

#[test]
fn hidden_commands_remain_invocable() {
    for command in HIDDEN_TOP_LEVEL {
        let (success, output) = run(&[command, "--help"]);
        assert!(
            success,
            "hidden command `{command}` must stay reachable:\n{output}"
        );
    }
}

#[test]
fn hidden_projects_subcommands_remain_invocable() {
    for command in HIDDEN_PROJECTS {
        let (success, output) = run(&["projects", command, "--help"]);
        assert!(
            success,
            "hidden command `projects {command}` must stay reachable:\n{output}"
        );
    }
}

#[test]
fn unknown_commands_still_fail() {
    let (success, output) = run(&["definitely-not-a-command"]);
    assert!(!success, "an unknown command must not succeed:\n{output}");
}
