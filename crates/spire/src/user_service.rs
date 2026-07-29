use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use spire_application::{InstallationProfile, ResolvedPaths};

pub fn install(paths: &ResolvedPaths, binary: &Path, yes: bool) -> Result<()> {
    if paths.profile != InstallationProfile::User {
        bail!(
            "`spire service install` only manages the user profile; use the system installer for --system"
        )
    }
    let unit_path = unit_path(paths)?;
    let rendered = render(paths, binary)?;
    if !yes {
        println!("{rendered}");
        println!(
            "preview only; rerun with `spire service install --yes` to write {}",
            unit_path.display()
        );
        return Ok(());
    }
    crate::runtime_paths::ensure_user_roots(paths)?;
    if let Ok(existing) = fs::read_to_string(&unit_path) {
        if existing != rendered {
            bail!(
                "refusing to overwrite modified user unit {}; inspect it or remove it explicitly",
                unit_path.display()
            )
        }
    } else {
        fs::create_dir_all(unit_path.parent().expect("unit path has parent")).with_context(
            || {
                format!(
                    "unable to create user unit directory {}",
                    unit_path.display()
                )
            },
        )?;
        fs::write(&unit_path, &rendered)
            .with_context(|| format!("unable to write user unit {}", unit_path.display()))?;
    }
    println!("user unit installed: {}", unit_path.display());
    daemon_reload()?;
    report_linger();
    Ok(())
}

pub fn systemctl(action: &str, system: bool) -> Result<()> {
    let mut command = Command::new("systemctl");
    if !system {
        command.arg("--user");
    }
    let status = command
        .args([action, "spire.service"])
        .status()
        .with_context(|| {
            format!(
                "failed to execute systemctl {}{action}",
                if system { "" } else { "--user " }
            )
        })?;
    if !status.success() {
        bail!(
            "systemctl {}{action} spire.service exited with {status}",
            if system { "" } else { "--user " }
        )
    }
    println!(
        "{} profile service {action} completed",
        if system { "system" } else { "user" }
    );
    Ok(())
}

fn daemon_reload() -> Result<()> {
    let status = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .context("failed to execute systemctl --user daemon-reload")?;
    if !status.success() {
        bail!("systemctl --user daemon-reload exited with {status}")
    }
    Ok(())
}

pub fn render(paths: &ResolvedPaths, binary: &Path) -> Result<String> {
    if !binary.is_absolute() || !paths.config_file.is_absolute() {
        bail!("user service requires absolute binary and configuration paths")
    }
    let binary = systemd_quote(binary)?;
    let config = systemd_quote(&paths.config_file)?;
    let writable = [
        systemd_quote(&paths.data_root)?,
        systemd_quote(&paths.state_root)?,
        systemd_quote(&paths.cache_root)?,
    ]
    .join(" ");
    Ok(format!(
        "[Unit]\nDescription=Spire orchestrator (user)\nAfter=network-online.target\nWants=network-online.target\nConditionPathExists={config}\n\n[Service]\nType=simple\nUMask=0077\nExecStartPre={binary} config validate --config {config}\nExecStart={binary} serve --config {config}\nRestart=on-failure\nRestartSec=5\nTimeoutStopSec=120\nKillSignal=SIGINT\nNoNewPrivileges=true\nPrivateTmp=true\nProtectSystem=strict\nProtectHome=read-only\nReadWritePaths={writable}\n\n[Install]\nWantedBy=default.target\n",
    ))
}

fn unit_path(paths: &ResolvedPaths) -> Result<PathBuf> {
    let xdg_config = paths
        .config_root
        .parent()
        .context("user config root must have an XDG parent")?;
    Ok(xdg_config.join("systemd/user/spire.service"))
}

fn systemd_quote(path: &Path) -> Result<String> {
    let value = path.to_str().context("unit paths must be valid UTF-8")?;
    if value.chars().any(char::is_control) {
        bail!("unit paths must not contain control characters")
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

fn report_linger() {
    let user = std::env::var("USER").unwrap_or_else(|_| "CURRENT_USER".to_owned());
    match Command::new("loginctl")
        .args(["show-user", &user, "-p", "Linger", "--value"])
        .output()
    {
        Ok(output)
            if output.status.success()
                && String::from_utf8_lossy(&output.stdout).trim() == "yes" =>
        {
            println!("user lingering is enabled; the service can persist after logout")
        }
        _ => println!(
            "logout/reboot persistence may require: loginctl enable-linger {user} (run this yourself with appropriate privilege)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendering_is_deterministic_and_quotes_spaces() {
        let paths = ResolvedPaths {
            profile: InstallationProfile::User,
            config_file: PathBuf::from("/home/alice/.config/spire/config file.yaml"),
            config_root: PathBuf::from("/home/alice/.config/spire"),
            data_root: PathBuf::from("/home/alice/.local/share/spire"),
            state_root: PathBuf::from("/home/alice/.local/state/spire"),
            cache_root: PathBuf::from("/home/alice/.cache/spire"),
        };
        let first = render(&paths, Path::new("/opt/spire/bin/spire tool")).unwrap();
        assert_eq!(
            first,
            render(&paths, Path::new("/opt/spire/bin/spire tool")).unwrap()
        );
        assert!(first.contains("\"/opt/spire/bin/spire tool\""));
        assert!(!first.contains("User="));
        assert!(!first.contains("credential"));
    }
}
