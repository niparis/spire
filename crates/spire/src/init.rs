//! Interactive first-run onboarding.
//!
//! Init writes nothing until every answer is collected, then performs one
//! atomic configuration write. An interrupted run therefore leaves the
//! installation exactly as it found it, and is restarted rather than resumed.
//! The resumable state machine in Sprint 15.1 is deliberately not implemented
//! here; see `docs/decisions/first-run-onboarding-and-project-mapping.md`.

use std::{
    collections::BTreeMap,
    fs,
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use spire_adapters::{
    linear::LinearReadAdapter,
    secrets::{UserAuthenticationMetadataStore, UserSecretStore},
};
use spire_application::{
    AuthenticationMetadataStorePort, AuthenticationState, Config, DEFAULT_TYPE_LABELS,
    ExternalResult, HarnessSelection, InstallationProfile, LinearAuthenticationMetadata,
    LinearOnboardingDiscoveryPort, LinearStateKind, LinearTeamConfiguration, LinearWorkflowState,
    ManagedSecret, OnboardingAnswers, ResolvedPaths, SecretStorePort, UNRESOLVED_FIELDS,
    rank_states, render_config, suggest_complexity_mapping,
};
use spire_domain::{Effort, HarnessId, ModelId};

use crate::runtime_paths;

/// The providers Spire can execute. `spire doctor` probes exactly these two.
const PROVIDERS: [(&str, &str); 2] = [("codex", "codex"), ("claude-code", "claude")];

pub async fn run(paths: ResolvedPaths, credential_file: Option<PathBuf>) -> Result<()> {
    if paths.profile != InstallationProfile::User {
        bail!("spire init provisions the user installation profile; --system is not supported")
    }
    if paths.config_file.exists() {
        bail!(
            "{} already exists; move it aside before re-running spire init",
            paths.config_file.display()
        )
    }
    if !std::io::stdin().is_terminal() {
        bail!("spire init is interactive and requires a terminal")
    }

    preview(&paths);
    if !confirm("Continue?")? {
        println!("nothing was written");
        return Ok(());
    }
    runtime_paths::ensure_user_roots(&paths)?;

    let (linear, organization_id, viewer_id) = authenticate(&paths, credential_file).await?;
    let team = select_team(&linear).await?;
    let state_ids = confirm_state_mapping(&team.states)?;
    let complexity_mapping = suggest_complexity_mapping(&team.estimates).with_context(|| {
        format!(
            "Linear team {} uses estimation type {}",
            team.team.key, team.estimates.kind
        )
    })?;

    println!("\nComplexity mapping derived from the team's estimate scale:");
    for (estimate, class) in &complexity_mapping {
        println!("  {} -> {class:?}", estimate.value());
    }
    if !confirm("Accept this mapping?")? {
        bail!("edit linear.complexity_mapping after init, or re-run once the team scale is correct")
    }

    let maker = select_harness("maker", None)?;
    let reviewer = select_harness("reviewer", Some(&maker.provider))?;

    let answers = OnboardingAnswers {
        organization_id,
        team_id: team.team.id.clone(),
        bot_actor_id: viewer_id,
        state_ids,
        complexity_mapping,
        type_labels: DEFAULT_TYPE_LABELS
            .iter()
            .map(|label| (*label).to_owned())
            .collect(),
        maker,
        reviewer,
    };
    let rendered = render_config(&answers, &paths)?;
    // A configuration Spire cannot parse back is a generator defect, not an
    // operator problem, so it must never reach the filesystem.
    Config::from_yaml(&rendered).context("spire init generated an unparseable configuration")?;
    write_new_config(&paths.config_file, rendered.as_bytes())?;

    report(&paths, &team);
    Ok(())
}

fn preview(paths: &ResolvedPaths) {
    println!("spire init will create or write:");
    for root in [
        &paths.config_root,
        &paths.data_root,
        &paths.state_root,
        &paths.cache_root,
    ] {
        println!("  {}", root.display());
    }
    println!("  {}", paths.config_file.display());
    println!("No Linear, GitHub, or Git change is made.");
}

/// Returns a verified Linear adapter plus the identities the configuration
/// needs. An already-configured credential is re-verified rather than trusted.
async fn authenticate(
    paths: &ResolvedPaths,
    credential_file: Option<PathBuf>,
) -> Result<(LinearReadAdapter, String, String)> {
    let store = UserSecretStore::below_config_root(&paths.config_root);
    let token = if store.status(ManagedSecret::LinearApiKey)? == AuthenticationState::Configured {
        println!("\nUsing the Linear credential already in the secret store.");
        store
            .read_for_service(ManagedSecret::LinearApiKey)
            .context("failed to load the stored Linear API key")?
    } else {
        println!("\nSpire needs a Linear API key with read access to your teams.");
        crate::read_secret_input(credential_file)?
    };

    let adapter = LinearReadAdapter::from_token(token.as_str().to_owned())
        .context("failed to construct the Linear client")?;
    let identity = adapter
        .verify_viewer()
        .await
        .context("Linear rejected the credential; nothing was stored")?;

    if store.status(ManagedSecret::LinearApiKey)? != AuthenticationState::Configured {
        store
            .replace(ManagedSecret::LinearApiKey, token)
            .context("failed to store the verified Linear credential")?;
    }
    let metadata_store = UserAuthenticationMetadataStore::below_config_root(&paths.config_root);
    let mut metadata = metadata_store.load()?;
    metadata.linear = Some(LinearAuthenticationMetadata {
        viewer_id: identity.viewer_id.clone(),
        organization_id: identity.organization_id.clone(),
        verified_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    });
    metadata_store.store(&metadata)?;

    println!(
        "Linear authenticated as viewer {} in organization {}.",
        identity.viewer_id, identity.organization_id
    );
    Ok((adapter, identity.organization_id, identity.viewer_id))
}

async fn select_team(linear: &LinearReadAdapter) -> Result<LinearTeamConfiguration> {
    let teams = linear
        .list_teams()
        .await
        .context("failed to list Linear teams")?;
    if teams.is_empty() {
        bail!("the authenticated Linear user can see no teams")
    }
    let labels: Vec<String> = teams
        .iter()
        .map(|team| format!("{} ({})", team.name, team.key))
        .collect();
    let index = choose("\nWhich Linear team does Spire work for?", &labels, None)?;

    match linear.team_configuration(&teams[index].id).await? {
        ExternalResult::Confirmed(configuration) => Ok(configuration),
        ExternalResult::NotFound => bail!("Linear no longer reports the selected team"),
        ExternalResult::Ambiguous { detail } => bail!("{detail}"),
    }
}

/// Binds each of Spire's seven lifecycle states to a Linear workflow state.
/// Suggestions are ordered but never applied without an explicit answer.
fn confirm_state_mapping(
    states: &[LinearWorkflowState],
) -> Result<BTreeMap<LinearStateKind, String>> {
    if states.is_empty() {
        bail!("the selected Linear team has no workflow states")
    }
    println!("\nMap each Spire lifecycle state to one Linear workflow state.");
    let labels: Vec<String> = states
        .iter()
        .map(|state| format!("{} [{:?}]", state.name, state.category))
        .collect();

    let mut mapping = BTreeMap::new();
    for kind in LinearStateKind::ALL {
        let suggested = rank_states(kind, states).first().copied();
        let index = choose(
            &format!("\nSpire `{}` is which Linear state?", kind.as_str()),
            &labels,
            suggested,
        )?;
        mapping.insert(kind, states[index].id.clone());
    }
    Ok(mapping)
}

fn select_harness(role: &str, exclude: Option<&HarnessId>) -> Result<HarnessSelection> {
    let available: Vec<&(&str, &str)> = PROVIDERS
        .iter()
        .filter(|(provider, _)| exclude.is_none_or(|taken| taken.as_str() != *provider))
        .collect();
    let labels: Vec<String> = available
        .iter()
        .map(|(provider, executable)| {
            let state = if on_path(executable) {
                "found on PATH"
            } else {
                "not found on PATH"
            };
            format!("{provider} ({state})")
        })
        .collect();
    let index = choose(
        &format!("\nWhich harness runs {role} work?"),
        &labels,
        Some(0),
    )?;
    let provider = HarnessId::new(available[index].0).map_err(anyhow::Error::msg)?;

    let model = prompt(&format!("Model ID for {role}: "))?;
    let model = ModelId::new(model.trim()).map_err(anyhow::Error::msg)?;
    let effort = match choose(
        &format!("Effort for {role}?"),
        &["low".to_owned(), "medium".to_owned(), "high".to_owned()],
        Some(2),
    )? {
        0 => Effort::Low,
        1 => Effort::Medium,
        _ => Effort::High,
    };
    Ok(HarnessSelection {
        provider,
        model,
        effort,
    })
}

fn on_path(executable: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| directory.join(executable).is_file())
    })
}

/// Creates the configuration file, refusing to follow or replace anything that
/// already occupies the path, and only renames a fully written file into place.
fn write_new_config(path: &Path, content: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let parent = path
        .parent()
        .context("configuration file must have a parent directory")?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("configuration path must have a UTF-8 file name")?;
    let temporary = parent.join(format!(".{file_name}.init.tmp"));
    let _ = fs::remove_file(&temporary);

    let write_result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .with_context(|| format!("failed to create {}", temporary.display()))?;
        file.write_all(content)?;
        file.sync_all()?;
        // `create_new` on the destination keeps a configuration that appeared
        // during the interview from being silently replaced.
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("{} already exists", path.display()))?;
        fs::rename(&temporary, path)
            .with_context(|| format!("failed to install {}", path.display()))?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

fn report(paths: &ResolvedPaths, team: &LinearTeamConfiguration) {
    println!(
        "\nWrote {} for Linear team {}.",
        paths.config_file.display(),
        team.team.key
    );
    println!("Linear writes remain disabled and no ticket can be admitted yet.\n");
    println!("Unresolved before `spire start`:");
    for (field, action) in UNRESOLVED_FIELDS {
        println!("  {field}: {action}");
    }
    println!("\n`spire config validate` names each one until it is resolved.");
    if cfg!(target_os = "macos") {
        println!(
            "On macOS there is no service to install; run `spire serve` in the foreground instead."
        );
    }
}

fn prompt(message: &str) -> Result<String> {
    print!("{message}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line)? == 0 {
        bail!("spire init was interrupted; nothing was written")
    }
    Ok(line.trim().to_owned())
}

fn confirm(message: &str) -> Result<bool> {
    Ok(matches!(
        prompt(&format!("{message} [y/N] "))?
            .to_ascii_lowercase()
            .as_str(),
        "y" | "yes"
    ))
}

/// Presents a numbered list and returns the chosen index. `suggested` is only a
/// default; the operator can always pick another entry.
fn choose(question: &str, options: &[String], suggested: Option<usize>) -> Result<usize> {
    println!("{question}");
    for (index, option) in options.iter().enumerate() {
        let marker = if Some(index) == suggested { "*" } else { " " };
        println!("  {marker} {}) {option}", index + 1);
    }
    loop {
        let hint = match suggested {
            Some(index) => format!("Choice [1-{}, default {}]: ", options.len(), index + 1),
            None => format!("Choice [1-{}]: ", options.len()),
        };
        let answer = prompt(&hint)?;
        if answer.is_empty()
            && let Some(index) = suggested
        {
            return Ok(index);
        }
        match answer.parse::<usize>() {
            Ok(choice) if (1..=options.len()).contains(&choice) => return Ok(choice - 1),
            _ => println!("Enter a number between 1 and {}.", options.len()),
        }
    }
}
