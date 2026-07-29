//! User-scoped `secrets.env` storage for Linear and GitHub App service credentials.
//!
//! This adapter intentionally does not serve system installations. A system
//! credential store has a different ownership and privilege contract.

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use nix::unistd::Uid;
use spire_application::{AuthenticationState, ManagedSecret, SecretInput, SecretStorePort};
use thiserror::Error;

const MAX_BUNDLE_BYTES: usize = 16 * 1024;
const MAX_VALUE_BYTES: usize = 4 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UserSecretStoreError {
    #[error("Spire secret bundle is unavailable")]
    Unavailable,
    #[error("Spire secret bundle is unsafe")]
    Unsafe,
    #[error("Spire secret bundle is malformed")]
    Malformed,
    #[error("Spire secret bundle is busy; retry after the concurrent rotation completes")]
    Busy,
    #[error("a secret value is invalid")]
    InvalidValue,
}

#[derive(Debug, Clone)]
pub struct UserSecretStore {
    path: PathBuf,
}

impl UserSecretStore {
    pub fn below_config_root(config_root: &Path) -> Self {
        Self {
            path: config_root.join("secrets.env"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read_bundle(&self) -> Result<BTreeMap<String, String>, UserSecretStoreError> {
        let path_metadata =
            fs::symlink_metadata(&self.path).map_err(|_| UserSecretStoreError::Unavailable)?;
        if path_metadata.file_type().is_symlink() {
            return Err(UserSecretStoreError::Unsafe);
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(&self.path)
            .map_err(|_| UserSecretStoreError::Unavailable)?;
        validate_file_metadata(
            &file
                .metadata()
                .map_err(|_| UserSecretStoreError::Unavailable)?,
        )?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(|_| UserSecretStoreError::Unavailable)?;
        parse_bundle(&contents)
    }

    fn acquire_lock(&self) -> Result<RotationLock, UserSecretStoreError> {
        let lock_path = self.path.with_extension("env.lock");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&lock_path)
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::AlreadyExists => UserSecretStoreError::Busy,
                _ => UserSecretStoreError::Unavailable,
            })?;
        Ok(RotationLock { path: lock_path })
    }

    fn write_bundle(&self, values: &BTreeMap<String, String>) -> Result<(), UserSecretStoreError> {
        let contents = serialize_bundle(values)?;
        let parent = self.path.parent().ok_or(UserSecretStoreError::Unsafe)?;
        validate_directory(parent)?;
        let temporary = parent.join(format!(
            ".secrets.env.{}.{}.tmp",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let write_result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
                .map_err(|_| UserSecretStoreError::Unavailable)?;
            file.write_all(contents.as_bytes())
                .map_err(|_| UserSecretStoreError::Unavailable)?;
            file.sync_all()
                .map_err(|_| UserSecretStoreError::Unavailable)?;
            fs::rename(&temporary, &self.path).map_err(|_| UserSecretStoreError::Unavailable)?;
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|_| UserSecretStoreError::Unavailable)
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result
    }
}

impl SecretStorePort for UserSecretStore {
    type Error = UserSecretStoreError;

    fn status(&self, secret: ManagedSecret) -> Result<AuthenticationState, Self::Error> {
        match self.read_bundle() {
            Ok(values) if values.contains_key(secret.key()) => Ok(AuthenticationState::Configured),
            Ok(_) | Err(UserSecretStoreError::Unavailable) => Ok(AuthenticationState::Unavailable),
            Err(error) => Err(error),
        }
    }

    fn replace(&self, secret: ManagedSecret, value: SecretInput) -> Result<(), Self::Error> {
        validate_value(value.as_str())?;
        let _lock = self.acquire_lock()?;
        let mut values = match self.read_bundle() {
            Ok(values) => values,
            Err(UserSecretStoreError::Unavailable) => BTreeMap::new(),
            Err(error) => return Err(error),
        };
        values.insert(secret.key().to_owned(), value.as_str().to_owned());
        self.write_bundle(&values)
    }

    fn remove(&self, secret: ManagedSecret) -> Result<(), Self::Error> {
        let _lock = self.acquire_lock()?;
        let mut values = self.read_bundle()?;
        values.remove(secret.key());
        self.write_bundle(&values)
    }
}

struct RotationLock {
    path: PathBuf,
}

impl Drop for RotationLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn validate_file_metadata(metadata: &fs::Metadata) -> Result<(), UserSecretStoreError> {
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(UserSecretStoreError::Unsafe);
    }
    if metadata.uid() != Uid::current().as_raw() || metadata.mode() & 0o777 != 0o600 {
        return Err(UserSecretStoreError::Unsafe);
    }
    if metadata.len() > MAX_BUNDLE_BYTES as u64 {
        return Err(UserSecretStoreError::Malformed);
    }
    Ok(())
}

fn validate_directory(path: &Path) -> Result<(), UserSecretStoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| UserSecretStoreError::Unavailable)?;
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != Uid::current().as_raw()
    {
        return Err(UserSecretStoreError::Unsafe);
    }
    Ok(())
}

fn parse_bundle(contents: &str) -> Result<BTreeMap<String, String>, UserSecretStoreError> {
    if contents.len() > MAX_BUNDLE_BYTES || contents.contains('\0') {
        return Err(UserSecretStoreError::Malformed);
    }
    let mut values = BTreeMap::new();
    for line in contents.lines() {
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or(UserSecretStoreError::Malformed)?;
        if !matches!(
            key,
            "LINEAR_API_KEY" | "GITHUB_APP_PRIVATE_KEY" | "GITHUB_WEBHOOK_SECRET"
        ) || values.contains_key(key)
        {
            return Err(UserSecretStoreError::Malformed);
        }
        validate_value(value)?;
        values.insert(key.to_owned(), value.to_owned());
    }
    Ok(values)
}

fn serialize_bundle(values: &BTreeMap<String, String>) -> Result<String, UserSecretStoreError> {
    let mut output = String::new();
    for (key, value) in values {
        if !matches!(
            key.as_str(),
            "LINEAR_API_KEY" | "GITHUB_APP_PRIVATE_KEY" | "GITHUB_WEBHOOK_SECRET"
        ) {
            return Err(UserSecretStoreError::Malformed);
        }
        validate_value(value)?;
        output.push_str(key);
        output.push('=');
        output.push_str(value);
        output.push('\n');
    }
    Ok(output)
}

fn validate_value(value: &str) -> Result<(), UserSecretStoreError> {
    if value.is_empty() || value.len() > MAX_VALUE_BYTES || value.contains(['\0', '\n', '\r']) {
        return Err(UserSecretStoreError::InvalidValue);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("spire-secret-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        root
    }

    #[test]
    fn rotations_are_atomic_and_preserve_unrelated_credentials() {
        let root = root("rotation");
        let store = UserSecretStore::below_config_root(&root);
        store
            .replace(
                ManagedSecret::LinearApiKey,
                SecretInput::new("linear-value".into()),
            )
            .unwrap();
        store
            .replace(
                ManagedSecret::GitHubAppPrivateKey,
                SecretInput::new("github-private-key".into()),
            )
            .unwrap();
        store
            .replace(
                ManagedSecret::GitHubWebhookSecret,
                SecretInput::new("github-webhook-secret".into()),
            )
            .unwrap();
        store.remove(ManagedSecret::LinearApiKey).unwrap();

        assert_eq!(
            store.status(ManagedSecret::LinearApiKey).unwrap(),
            AuthenticationState::Unavailable
        );
        assert_eq!(
            store.status(ManagedSecret::GitHubAppPrivateKey).unwrap(),
            AuthenticationState::Configured
        );
        assert_eq!(
            store.status(ManagedSecret::GitHubWebhookSecret).unwrap(),
            AuthenticationState::Configured
        );
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_permissions_and_symlinks_fail_closed() {
        let root = root("unsafe");
        let store = UserSecretStore::below_config_root(&root);
        fs::write(store.path(), "LINEAR_API_KEY=value\n").unwrap();
        fs::set_permissions(store.path(), fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            store.status(ManagedSecret::LinearApiKey),
            Err(UserSecretStoreError::Unsafe)
        );
        fs::remove_file(store.path()).unwrap();
        std::os::unix::fs::symlink("/tmp", store.path()).unwrap();
        assert_eq!(
            store.status(ManagedSecret::LinearApiKey),
            Err(UserSecretStoreError::Unsafe)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_values_are_rejected_without_exposing_them() {
        let root = root("parse");
        let store = UserSecretStore::below_config_root(&root);
        fs::write(
            store.path(),
            "LINEAR_API_KEY=first\nLINEAR_API_KEY=SPIRE_SECRET_SENTINEL\n",
        )
        .unwrap();
        fs::set_permissions(store.path(), fs::Permissions::from_mode(0o600)).unwrap();
        let error = store.status(ManagedSecret::LinearApiKey).unwrap_err();
        assert_eq!(error, UserSecretStoreError::Malformed);
        assert!(!error.to_string().contains("SPIRE_SECRET_SENTINEL"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn generic_github_credential_records_are_rejected() {
        let root = root("legacy-github-key");
        let store = UserSecretStore::below_config_root(&root);
        fs::write(store.path(), "GITHUB_CREDENTIAL=SPIRE_SECRET_SENTINEL\n").unwrap();
        fs::set_permissions(store.path(), fs::Permissions::from_mode(0o600)).unwrap();

        let error = store
            .status(ManagedSecret::GitHubAppPrivateKey)
            .unwrap_err();

        assert_eq!(error, UserSecretStoreError::Malformed);
        assert!(!error.to_string().contains("SPIRE_SECRET_SENTINEL"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn concurrent_rotation_fails_closed_instead_of_losing_a_write() {
        let root = root("lock");
        let store = UserSecretStore::below_config_root(&root);
        let lock = store.acquire_lock().unwrap();
        assert_eq!(
            store.replace(
                ManagedSecret::LinearApiKey,
                SecretInput::new("value".into())
            ),
            Err(UserSecretStoreError::Busy)
        );
        drop(lock);
        let _ = fs::remove_dir_all(root);
    }
}
