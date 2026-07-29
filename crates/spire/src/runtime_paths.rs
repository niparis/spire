use std::{
    collections::BTreeMap,
    env, fs, io,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use spire_application::{InstallationProfile, ResolvedPaths};

const SPIRE_CONFIG: &str = "SPIRE_CONFIG";

/// Resolves every Spire root from one profile.  The caller supplies all
/// environment access, keeping this logic deterministic in tests.
pub fn resolve_paths(
    explicit_config: Option<&Path>,
    system: bool,
    environment: &BTreeMap<String, String>,
) -> Result<ResolvedPaths> {
    if system {
        if explicit_config.is_some() || environment.contains_key(SPIRE_CONFIG) {
            bail!("--system cannot be combined with --config or SPIRE_CONFIG")
        }
        return Ok(ResolvedPaths::system());
    }

    let home = environment
        .get("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .context("HOME is required to resolve the user installation profile")?;
    if !home.is_absolute() {
        bail!("HOME must be an absolute path")
    }
    let config_root = xdg_root(environment, "XDG_CONFIG_HOME", home.join(".config"))?.join("spire");
    let data_root =
        xdg_root(environment, "XDG_DATA_HOME", home.join(".local/share"))?.join("spire");
    let state_root =
        xdg_root(environment, "XDG_STATE_HOME", home.join(".local/state"))?.join("spire");
    let cache_root = xdg_root(environment, "XDG_CACHE_HOME", home.join(".cache"))?.join("spire");
    validate_distinct_roots([
        config_root.as_path(),
        data_root.as_path(),
        state_root.as_path(),
        cache_root.as_path(),
    ])?;

    let configured = explicit_config
        .map(PathBuf::from)
        .or_else(|| environment.get(SPIRE_CONFIG).map(PathBuf::from));
    if let Some(config_file) = configured {
        return resolve_explicit_user_paths(
            config_file,
            config_root,
            data_root,
            state_root,
            cache_root,
        );
    }

    let config_file = config_root.join("config.yaml");
    let system_config = ResolvedPaths::system().config_file;
    if config_file.exists() && system_config.exists() {
        bail!(
            "both user configuration {} and system configuration {} exist; select --system or --config explicitly",
            config_file.display(),
            system_config.display()
        )
    }
    if !config_file.exists() && system_config.exists() {
        return Ok(ResolvedPaths::system());
    }
    resolve_explicit_user_paths(config_file, config_root, data_root, state_root, cache_root)
}

pub fn process_environment() -> BTreeMap<String, String> {
    env::vars().collect()
}

pub fn ensure_user_roots(paths: &ResolvedPaths) -> Result<()> {
    if paths.profile != InstallationProfile::User {
        bail!("creating system-profile paths requires the explicit system installer")
    }
    let roots = [
        &paths.config_root,
        &paths.data_root,
        &paths.state_root,
        &paths.cache_root,
    ];
    let mut created = Vec::new();
    for root in roots {
        if !root.exists() {
            fs::create_dir_all(root)
                .with_context(|| format!("unable to create Spire directory {}", root.display()))?;
            created.push(root.clone());
        }
        if let Err(error) =
            set_private_permissions(root).and_then(|_| ensure_current_user_owns(root))
        {
            for path in created.iter().rev() {
                let _ = fs::remove_dir(path);
            }
            return Err(error);
        }
    }
    Ok(())
}

fn resolve_explicit_user_paths(
    config_file: PathBuf,
    config_root: PathBuf,
    data_root: PathBuf,
    state_root: PathBuf,
    cache_root: PathBuf,
) -> Result<ResolvedPaths> {
    if !config_file.is_absolute() {
        bail!("configuration path must be absolute")
    }
    if config_file.file_name().is_none() {
        bail!("configuration path must name a file")
    }
    reject_symlink_components(&config_file)?;
    for root in [&config_root, &data_root, &state_root, &cache_root] {
        reject_symlink_components(root)?;
        ensure_existing_parent_is_directory(root)?;
        ensure_current_user_owns_existing_parent(root)?;
    }
    ensure_existing_parent_is_directory(&config_file)?;
    ensure_current_user_owns_existing_parent(&config_file)?;
    Ok(ResolvedPaths {
        profile: InstallationProfile::User,
        config_file,
        config_root,
        data_root,
        state_root,
        cache_root,
    })
}

fn xdg_root(
    environment: &BTreeMap<String, String>,
    key: &str,
    fallback: PathBuf,
) -> Result<PathBuf> {
    let root = environment.get(key).map(PathBuf::from).unwrap_or(fallback);
    if !root.is_absolute() {
        bail!("{key} must be an absolute path")
    }
    Ok(root)
}

fn validate_distinct_roots<'a>(roots: impl IntoIterator<Item = &'a Path>) -> Result<()> {
    let roots = roots.into_iter().collect::<Vec<_>>();
    for (index, first) in roots.iter().enumerate() {
        for second in roots.iter().skip(index + 1) {
            if first == second || first.starts_with(second) || second.starts_with(first) {
                bail!(
                    "Spire XDG roots must be distinct and non-overlapping: {} and {}",
                    first.display(),
                    second.display()
                )
            }
        }
    }
    Ok(())
}

fn ensure_existing_parent_is_directory(path: &Path) -> Result<()> {
    let parent = existing_parent(path)?;
    if !parent.is_dir() {
        bail!("existing parent {} is not a directory", parent.display())
    }
    Ok(())
}

fn ensure_current_user_owns_existing_parent(path: &Path) -> Result<()> {
    ensure_current_user_owns(&existing_parent(path)?)
}

fn existing_parent(path: &Path) -> Result<PathBuf> {
    let mut current = path;
    while !current.exists() {
        current = current.parent().context("path has no existing parent")?;
    }
    Ok(current.to_path_buf())
}

fn reject_symlink_components(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(part) => {
                current.push(part);
                match fs::symlink_metadata(&current) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        bail!("Spire path must not traverse symlink {}", current.display())
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(error)
                            .with_context(|| format!("unable to inspect {}", current.display()));
                    }
                }
            }
            Component::CurDir | Component::ParentDir => bail!("Spire path must be normalized"),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn ensure_current_user_owns(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let owner = fs::metadata(path)
        .with_context(|| format!("unable to inspect {}", path.display()))?
        .uid();
    let current = nix::unistd::Uid::current().as_raw();
    if owner != current {
        bail!("Spire path {} is owned by a different user", path.display())
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_current_user_owns(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("unable to set private permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(home: &str) -> BTreeMap<String, String> {
        BTreeMap::from([("HOME".to_owned(), home.to_owned())])
    }

    fn temporary_home() -> PathBuf {
        let path = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("spire-path-test-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn uses_xdg_fallbacks() {
        let home = temporary_home();
        let paths = resolve_paths(None, false, &environment(home.to_str().unwrap())).unwrap();
        assert_eq!(paths.config_file, home.join(".config/spire/config.yaml"));
        assert_eq!(paths.data_root, home.join(".local/share/spire"));
    }

    #[test]
    fn explicit_config_wins_over_environment() {
        let home = temporary_home();
        let mut environment = environment(home.to_str().unwrap());
        environment.insert(
            SPIRE_CONFIG.to_owned(),
            home.join("from-env.yaml").display().to_string(),
        );
        let explicit = home.join("explicit.yaml");
        let paths = resolve_paths(Some(&explicit), false, &environment).unwrap();
        assert_eq!(paths.config_file, explicit);
    }

    #[test]
    fn environment_config_wins_over_xdg_default() {
        let home = temporary_home();
        let mut environment = environment(home.to_str().unwrap());
        let from_environment = home.join("from-environment.yaml");
        environment.insert(
            SPIRE_CONFIG.to_owned(),
            from_environment.display().to_string(),
        );
        let paths = resolve_paths(None, false, &environment).unwrap();
        assert_eq!(paths.config_file, from_environment);
    }

    #[test]
    fn system_profile_rejects_user_overrides() {
        let environment = environment("/irrelevant");
        assert!(resolve_paths(Some(Path::new("/tmp/config.yaml")), true, &environment).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let home = temporary_home();
        let target = home.join("target");
        let link = home.join("linked");
        fs::create_dir_all(&target).unwrap();
        let _ = fs::remove_file(&link);
        symlink(&target, &link).unwrap();
        let mut environment = environment(home.to_str().unwrap());
        environment.insert(
            SPIRE_CONFIG.to_owned(),
            link.join("config.yaml").display().to_string(),
        );
        assert!(resolve_paths(None, false, &environment).is_err());
    }

    #[test]
    fn rejects_relative_and_conflicting_overrides() {
        let mut environment = environment("/users/alice");
        environment.insert(SPIRE_CONFIG.to_owned(), "relative.yaml".to_owned());
        assert!(resolve_paths(None, false, &environment).is_err());
        environment.insert(
            "XDG_DATA_HOME".to_owned(),
            "/users/alice/.config".to_owned(),
        );
        environment.remove(SPIRE_CONFIG);
        assert!(resolve_paths(None, false, &environment).is_err());
    }
}
