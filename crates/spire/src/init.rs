//! Interactive, re-runnable onboarding.
//!
//! `init` owns the session boundary: authentication and discovery are prepared,
//! the push-based editor mutates an in-memory model, and only a committed model
//! reaches the atomic configuration replacement. Authentication metadata is
//! deliberately deferred until that same commit path.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use spire_adapters::{
    linear::LinearReadAdapter,
    secrets::{UserAuthenticationMetadataStore, UserSecretStore},
};
use spire_application::{
    AuthenticationMetadataStorePort, AuthenticationState, Config, InstallationProfile,
    LinearAuthenticationMetadata, ManagedSecret, OnboardingEditorPort, OnboardingEditorResult,
    OnboardingModel, ResolvedPaths, SecretInput, SecretStorePort, UNRESOLVED_FIELDS,
};

use crate::{onboarding_editor, runtime_paths};

struct AuthenticatedSession {
    adapter: LinearReadAdapter,
    organization_id: String,
    viewer_id: String,
    token: SecretInput,
    persist_token: bool,
}

pub async fn run(paths: ResolvedPaths, credential_file: Option<PathBuf>) -> Result<()> {
    if paths.profile != InstallationProfile::User {
        bail!("spire init provisions the user installation profile; --system is not supported")
    }
    runtime_paths::ensure_user_roots(&paths)?;

    // Parse before entering the alternate screen. An invalid existing document
    // is an actionable startup error, not a half-rendered editor state.
    let existing = if paths.config_file.exists() {
        let contents = fs::read_to_string(&paths.config_file).with_context(|| {
            format!(
                "unable to read existing configuration {}",
                paths.config_file.display()
            )
        })?;
        let config = Config::from_yaml(&contents).with_context(|| {
            format!(
                "existing configuration {} is malformed",
                paths.config_file.display()
            )
        })?;
        Some((contents, config))
    } else {
        None
    };

    // The catalog is loaded before alternate-screen entry, as a missing or
    // malformed catalog must name its path and leave the terminal untouched.
    let catalog = onboarding_editor::load_default_model_catalog(&paths)?;
    onboarding_editor::validate_terminal()?;
    let authenticated = authenticate(&paths, credential_file).await?;
    let mut model = existing
        .as_ref()
        .map(|(_, config)| OnboardingModel::from_config(config))
        .unwrap_or_else(OnboardingModel::empty);
    model.credential_verified = true;
    model.organization_id.value = Some(authenticated.organization_id.clone());
    model.bot_actor_id.value = Some(authenticated.viewer_id.clone());
    model.catalog_version = Some(catalog.version.clone());
    seed_default_harnesses(&mut model, &catalog)?;

    let AuthenticatedSession {
        adapter,
        organization_id,
        viewer_id,
        token,
        persist_token,
    } = authenticated;
    let (request_tx, response_rx) = onboarding_editor::spawn_discovery(adapter);
    let mut editor = onboarding_editor::TerminalOnboardingEditor::new(
        paths.clone(),
        catalog,
        request_tx,
        response_rx,
    );
    let result = editor.edit(model, Default::default())?;
    let OnboardingEditorResult::Committed(model) = result else {
        println!("onboarding abandoned; nothing was written");
        return Ok(());
    };
    let model = *model;

    let existing_contents = existing.as_ref().map(|(contents, _)| contents.as_str());
    let rendered = spire_application::render_model_config(&model, &paths, existing_contents)
        .map_err(|error| anyhow::anyhow!("cannot write onboarding model: {error}"))?;
    Config::from_yaml(&rendered).context("spire init generated an unparseable configuration")?;

    let replacement = replace_config_with_backup(&paths.config_file, rendered.as_bytes())?;
    if let Err(error) = commit_authentication(
        &paths,
        organization_id,
        viewer_id,
        token,
        persist_token,
        &replacement,
    ) {
        replacement.restore().ok();
        return Err(error);
    }
    onboarding_editor::append_write_trace(
        &paths.state_root.join("onboarding-trace.jsonl"),
        &paths.config_file,
        replacement.backup.as_deref(),
    )?;
    report(&paths, replacement.backup.as_deref());
    Ok(())
}

fn seed_default_harnesses(
    model: &mut OnboardingModel,
    catalog: &onboarding_editor::ModelCatalog,
) -> Result<()> {
    if model.maker.value.is_none() {
        let provider = spire_domain::HarnessId::new("codex")?;
        let model_id = catalog
            .models_for(&provider)
            .into_iter()
            .next()
            .context("model catalog has no codex model")?;
        model.maker.value = Some(spire_application::HarnessSelection {
            provider,
            model: model_id,
            effort: spire_domain::Effort::High,
        });
        model.maker_model_confirmed = true;
    }
    if model.reviewer.value.is_none() {
        let provider = spire_domain::HarnessId::new("claude-code")?;
        let model_id = catalog
            .models_for(&provider)
            .into_iter()
            .next()
            .context("model catalog has no claude-code model")?;
        model.reviewer.value = Some(spire_application::HarnessSelection {
            provider,
            model: model_id,
            effort: spire_domain::Effort::High,
        });
        model.reviewer_model_confirmed = true;
    }
    let maker_off_catalog = model.maker.value.as_ref().is_some_and(|selection| {
        !catalog
            .models_for(&selection.provider)
            .contains(&selection.model)
    });
    let reviewer_off_catalog = model.reviewer.value.as_ref().is_some_and(|selection| {
        !catalog
            .models_for(&selection.provider)
            .contains(&selection.model)
    });
    if maker_off_catalog {
        model.set_model_catalog_state(spire_application::OnboardingRole::Maker, true);
    }
    if reviewer_off_catalog {
        model.set_model_catalog_state(spire_application::OnboardingRole::Reviewer, true);
    }
    Ok(())
}

/// Returns a verified adapter and keeps the credential in memory until the
/// editor commits. No secret-store or metadata write happens here.
async fn authenticate(
    paths: &ResolvedPaths,
    credential_file: Option<PathBuf>,
) -> Result<AuthenticatedSession> {
    let store = UserSecretStore::below_config_root(&paths.config_root);
    let configured = store.status(ManagedSecret::LinearApiKey)? == AuthenticationState::Configured;
    let token = if configured {
        println!("Using the Linear credential already in the secret store.");
        store
            .read_for_service(ManagedSecret::LinearApiKey)
            .context("failed to load the stored Linear API key")?
    } else {
        println!("Spire needs a Linear API key with read access to your teams.");
        crate::read_secret_input(credential_file)?
    };
    let adapter = LinearReadAdapter::from_token(token.as_str().to_owned())
        .context("failed to construct the Linear client")?;
    let identity = adapter
        .verify_viewer()
        .await
        .context("Linear rejected the credential; nothing was stored")?;
    println!(
        "Linear authenticated as viewer {} in organization {}.",
        identity.viewer_id, identity.organization_id
    );
    Ok(AuthenticatedSession {
        adapter,
        organization_id: identity.organization_id,
        viewer_id: identity.viewer_id,
        token,
        persist_token: !configured,
    })
}

fn commit_authentication(
    paths: &ResolvedPaths,
    organization_id: String,
    viewer_id: String,
    token: SecretInput,
    persist_token: bool,
    replacement: &ConfigReplacement,
) -> Result<()> {
    let store = UserSecretStore::below_config_root(&paths.config_root);
    let metadata_store = UserAuthenticationMetadataStore::below_config_root(&paths.config_root);
    // Read the prior metadata before replacing a newly supplied token. This
    // keeps a metadata-read failure from leaving a secret behind after the
    // configuration replacement is rolled back.
    let previous = metadata_store.load()?;
    if persist_token && let Err(error) = store.replace(ManagedSecret::LinearApiKey, token) {
        replacement.restore().ok();
        return Err(error.into());
    }
    let mut metadata = previous.clone();
    metadata.linear = Some(LinearAuthenticationMetadata {
        viewer_id,
        organization_id,
        verified_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    });
    if let Err(error) = metadata_store.store(&metadata) {
        metadata_store.store(&previous).ok();
        if persist_token {
            store.remove(ManagedSecret::LinearApiKey).ok();
        }
        replacement.restore().ok();
        return Err(error.into());
    }
    Ok(())
}

struct ConfigReplacement {
    path: PathBuf,
    backup: Option<PathBuf>,
    had_original: bool,
}

impl ConfigReplacement {
    fn restore(&self) -> Result<()> {
        match (&self.backup, self.had_original) {
            (Some(backup), true) => {
                fs::copy(backup, &self.path).with_context(|| {
                    format!(
                        "failed to restore configuration backup {}",
                        backup.display()
                    )
                })?;
            }
            (None, false) => {
                let _ = fs::remove_file(&self.path);
            }
            _ => {}
        }
        Ok(())
    }
}

fn replace_config_with_backup(path: &Path, content: &[u8]) -> Result<ConfigReplacement> {
    let parent = path
        .parent()
        .context("configuration file must have a parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("unable to create {}", parent.display()))?;
    let had_original = path.exists();
    let backup = if had_original {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("configuration path must have a UTF-8 file name")?;
        let backup = parent.join(format!("{file_name}.before-init-{timestamp}.bak"));
        fs::copy(path, &backup).with_context(|| {
            format!("failed to create configuration backup {}", backup.display())
        })?;
        Some(backup)
    } else {
        None
    };
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("configuration path must have a UTF-8 file name")?;
    let temporary = parent.join(format!(".{file_name}.init.tmp"));
    let _ = fs::remove_file(&temporary);
    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .with_context(|| format!("failed to create {}", temporary.display()))?;
        file.write_all(content)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
            .with_context(|| format!("failed to install {}", path.display()))?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(ConfigReplacement {
        path: path.to_owned(),
        backup,
        had_original,
    })
}

fn report(paths: &ResolvedPaths, backup: Option<&Path>) {
    println!("\nWrote {}.", paths.config_file.display());
    if let Some(backup) = backup {
        println!("Previous configuration backed up at {}.", backup.display());
    }
    println!(
        "Trace: {}",
        paths.state_root.join("onboarding-trace.jsonl").display()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_creates_a_backup_and_can_restore_the_original() {
        let root =
            std::env::temp_dir().join(format!("spire-init-replacement-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.yaml");
        fs::write(&path, b"old configuration\n").unwrap();

        let replacement = replace_config_with_backup(&path, b"new configuration\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new configuration\n");
        assert!(
            replacement
                .backup
                .as_ref()
                .is_some_and(|backup| backup.exists())
        );
        replacement.restore().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "old configuration\n");
        assert!(
            replacement
                .backup
                .as_ref()
                .is_some_and(|backup| backup.exists())
        );
        let _ = fs::remove_dir_all(&root);
    }
}
