use serde::Serialize;
use serde_yaml::{Mapping, Value};
use spire_domain::{ComplexityClass, DispatchCandidate, RunRole};
use thiserror::Error;

use crate::{DispatchRuleConfig, HarnessRoleConfig, SCHEMA_VERSION};

#[derive(Debug, Clone, Serialize)]
pub struct ConfigMigrationPreview {
    pub from_schema_version: u32,
    pub to_schema_version: u32,
    pub deferred_fields: Vec<String>,
    pub configuration: Value,
}

#[derive(Debug, Error)]
pub enum ConfigMigrationError {
    #[error("invalid configuration YAML: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("config migration only accepts schema_version 3")]
    UnsupportedSchema,
    #[error("schema 3 dispatch has no unambiguous {role:?} selection for {complexity:?}: {detail}")]
    AmbiguousDispatch {
        role: RunRole,
        complexity: ComplexityClass,
        detail: String,
    },
    #[error("schema 3 maker and reviewer resolve to the same provider")]
    SameProvider,
    #[error("schema 3 configuration is missing {0}")]
    Missing(&'static str),
}

/// Converts only the unambiguous role-shaped subset of schema 3.  It never
/// changes a file; the CLI owns preview redaction, backups, and replacement.
pub fn preview_schema3_migration(
    input: &str,
) -> Result<ConfigMigrationPreview, ConfigMigrationError> {
    let mut value = serde_yaml::from_str::<Value>(input)?;
    let document = value
        .as_mapping_mut()
        .ok_or(ConfigMigrationError::Missing("top-level mapping"))?;
    if mapping_u32(document, "schema_version") != Some(3) {
        return Err(ConfigMigrationError::UnsupportedSchema);
    }
    let dispatch = mapping_value(document, "dispatch")
        .and_then(Value::as_mapping)
        .ok_or(ConfigMigrationError::Missing("dispatch"))?;
    let rules = mapping_value(dispatch, "rules")
        .and_then(Value::as_sequence)
        .ok_or(ConfigMigrationError::Missing("dispatch.rules"))?
        .iter()
        .cloned()
        .map(serde_yaml::from_value::<DispatchRuleConfig>)
        .collect::<Result<Vec<_>, _>>()?;

    let maker = resolved_role(RunRole::Implementation, &rules)?;
    let reviewer = resolved_role(RunRole::Review, &rules)?;
    if maker.provider == reviewer.provider {
        return Err(ConfigMigrationError::SameProvider);
    }

    document.insert(key("schema_version"), Value::Number(SCHEMA_VERSION.into()));
    document.remove(key("dispatch"));
    document.insert(
        key("harnesses"),
        serde_yaml::to_value(serde_json::json!({
            "maker": role_value(&maker),
            "reviewer": role_value(&reviewer),
        }))?,
    );

    Ok(ConfigMigrationPreview {
        from_schema_version: 3,
        to_schema_version: SCHEMA_VERSION,
        deferred_fields: vec![
            "harnesses.*.credential_ref (provider-native authentication is configured in Sprint 13)".to_owned(),
            "linear.repository_mappings (SQLite-backed routing migrates in Sprint 14)".to_owned(),
        ],
        configuration: value,
    })
}

fn resolved_role(
    role: RunRole,
    rules: &[DispatchRuleConfig],
) -> Result<HarnessRoleConfig, ConfigMigrationError> {
    let mut selected = None;
    for complexity in ComplexityClass::ALL {
        let matching = rules
            .iter()
            .filter(|rule| rule.when.role == role && rule.when.complexity.contains(&complexity))
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(ConfigMigrationError::AmbiguousDispatch {
                role,
                complexity,
                detail: format!("{} matching rules", matching.len()),
            });
        }
        let candidate = matching[0].candidates.first().ok_or_else(|| {
            ConfigMigrationError::AmbiguousDispatch {
                role,
                complexity,
                detail: "matching rule has no candidates".to_owned(),
            }
        })?;
        match &selected {
            Some(current) if current != candidate => {
                return Err(ConfigMigrationError::AmbiguousDispatch {
                    role,
                    complexity,
                    detail: "selected candidate changes by complexity".to_owned(),
                });
            }
            Some(_) => {}
            None => selected = Some(candidate.clone()),
        }
    }
    let DispatchCandidate {
        harness,
        model,
        effort,
    } = selected.expect("all complexity classes examined");
    Ok(HarnessRoleConfig {
        provider: harness,
        model,
        effort,
    })
}

fn role_value(role: &HarnessRoleConfig) -> serde_json::Value {
    serde_json::json!({
        "provider": role.provider.as_str(),
        "model": role.model.as_str(),
        "effort": role.effort,
    })
}

fn key(name: &str) -> Value {
    Value::String(name.to_owned())
}

fn mapping_value<'a>(mapping: &'a Mapping, name: &str) -> Option<&'a Value> {
    mapping.get(key(name))
}

fn mapping_u32(mapping: &Mapping, name: &str) -> Option<u32> {
    mapping_value(mapping, name)?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA3: &str = r#"
schema_version: 3
harnesses:
  codex: {executable: codex, credential_ref: env:CODEX_TOKEN, models: [codex-model], efforts: [high]}
  claude-code: {executable: claude, credential_ref: env:CLAUDE_TOKEN, models: [claude-model], efforts: [high]}
dispatch:
  policy_version: 1
  rules:
    - {id: maker, when: {role: implementation, complexity: [small, medium, large, xlarge]}, candidates: [{harness: codex, model: codex-model, effort: high}]}
    - {id: reviewer, when: {role: review, complexity: [small, medium, large, xlarge]}, candidates: [{harness: claude-code, model: claude-model, effort: high}]}
rollout: {linear_writes_enabled: false}
"#;

    #[test]
    fn migration_is_deterministic_and_defers_legacy_fields() {
        let first = preview_schema3_migration(SCHEMA3).unwrap();
        let second = preview_schema3_migration(SCHEMA3).unwrap();
        assert_eq!(
            serde_yaml::to_string(&first.configuration).unwrap(),
            serde_yaml::to_string(&second.configuration).unwrap()
        );
        assert_eq!(first.configuration["schema_version"].as_u64(), Some(4));
        assert_eq!(
            first.configuration["harnesses"]["maker"]["provider"].as_str(),
            Some("codex")
        );
        assert_eq!(first.deferred_fields.len(), 2);
    }

    #[test]
    fn migration_rejects_complexity_specific_selection() {
        let input = SCHEMA3.replace(
            "complexity: [small, medium, large, xlarge]",
            "complexity: [small]",
        );
        assert!(matches!(
            preview_schema3_migration(&input),
            Err(ConfigMigrationError::AmbiguousDispatch { .. })
        ));
    }
}
