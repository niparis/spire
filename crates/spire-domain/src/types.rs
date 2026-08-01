use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use uuid::Uuid;

const MAX_VALUE_LENGTH: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValueError {
    #[error("{kind} must not be empty")]
    Empty { kind: &'static str },
    #[error("{kind} must not exceed {MAX_VALUE_LENGTH} characters")]
    TooLong { kind: &'static str },
    #[error("{kind} is invalid: {value}")]
    Invalid { kind: &'static str, value: String },
}

fn checked_value(kind: &'static str, value: impl Into<String>) -> Result<String, ValueError> {
    let value = value.into();
    if value.trim().is_empty() {
        return Err(ValueError::Empty { kind });
    }
    if value.len() > MAX_VALUE_LENGTH {
        return Err(ValueError::TooLong { kind });
    }
    Ok(value)
}

macro_rules! string_value {
    ($name:ident, $kind:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
                Ok(Self(checked_value($kind, value)?))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = ValueError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                String::deserialize(deserializer)
                    .and_then(|value| Self::new(value).map_err(serde::de::Error::custom))
            }
        }
    };
}

macro_rules! uuid_value {
    ($name:ident, $kind:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn parse(value: &str) -> Result<Self, ValueError> {
                Uuid::parse_str(value)
                    .map(Self)
                    .map_err(|_| ValueError::Invalid {
                        kind: $kind,
                        value: value.to_owned(),
                    })
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                String::deserialize(deserializer)
                    .and_then(|value| Self::parse(&value).map_err(serde::de::Error::custom))
            }
        }
    };
}

uuid_value!(WorkItemId, "work item ID");
uuid_value!(RunId, "run ID");
uuid_value!(ReviewCycleId, "review cycle ID");
uuid_value!(WorkspaceId, "workspace ID");
uuid_value!(ProjectRepositoryMappingId, "project repository mapping ID");

string_value!(LinearIssueId, "Linear issue ID");
string_value!(LinearIdentifier, "Linear issue identifier");
string_value!(LinearProjectId, "Linear project ID");
string_value!(RepositoryName, "repository name");
string_value!(CommitSha, "commit SHA");
string_value!(HarnessId, "harness ID");
string_value!(ModelId, "model ID");
string_value!(CredentialProfile, "credential profile");
string_value!(DispatchRuleId, "dispatch rule ID");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ComplexityEstimate(u8);

impl ComplexityEstimate {
    pub fn new(value: u8) -> Result<Self, ValueError> {
        if value == 0 {
            return Err(ValueError::Invalid {
                kind: "complexity estimate",
                value: value.to_string(),
            });
        }
        Ok(Self(value))
    }

    pub fn value(self) -> u8 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ComplexityEstimate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        u8::deserialize(deserializer)
            .and_then(|value| Self::new(value).map_err(serde::de::Error::custom))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplexityClass {
    Small,
    Medium,
    Large,
    Xlarge,
}

impl ComplexityClass {
    pub const ALL: [Self; 4] = [Self::Small, Self::Medium, Self::Large, Self::Xlarge];
}

/// Ordered from least to most reasoning budget; `Ord` is relied upon to compare
/// an effort against a model's ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
    Max,
    Ultra,
}

impl Effort {
    pub const ALL: [Self; 6] = [
        Self::Low,
        Self::Medium,
        Self::High,
        Self::XHigh,
        Self::Max,
        Self::Ultra,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
            Self::Ultra => "ultra",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ValueError> {
        Self::ALL
            .into_iter()
            .find(|effort| effort.as_str() == value)
            .ok_or_else(|| ValueError::Invalid {
                kind: "effort",
                value: value.to_string(),
            })
    }
}

impl fmt::Display for Effort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct DispatchPolicyVersion(u32);

impl DispatchPolicyVersion {
    pub fn new(value: u32) -> Result<Self, ValueError> {
        if value == 0 {
            return Err(ValueError::Invalid {
                kind: "dispatch policy version",
                value: value.to_string(),
            });
        }
        Ok(Self(value))
    }

    pub fn value(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ProjectMappingRevision(u64);

impl ProjectMappingRevision {
    pub fn new(value: u64) -> Result<Self, ValueError> {
        if value == 0 {
            return Err(ValueError::Invalid {
                kind: "project mapping revision",
                value: value.to_string(),
            });
        }
        Ok(Self(value))
    }

    pub fn value(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl<'de> Deserialize<'de> for ProjectMappingRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        u64::deserialize(deserializer)
            .and_then(|value| Self::new(value).map_err(serde::de::Error::custom))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectMappingStatus {
    Enabled,
    Disabled,
    Removed,
}

impl<'de> Deserialize<'de> for DispatchPolicyVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        u32::deserialize(deserializer)
            .and_then(|value| Self::new(value).map_err(serde::de::Error::custom))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CandidateIndex(usize);

impl CandidateIndex {
    pub fn new(value: usize) -> Self {
        Self(value)
    }

    pub fn value(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Initiator {
    Human,
    Ai,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    LinearReady,
    OperatorRetry,
    CiFailed,
    ReviewRequired,
    ReviewChangesRequested,
    ProviderCapacityContinuation,
    ReconciliationRecovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunRole {
    Implementation,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapacity {
    Available,
    RateLimited,
    QuotaExhausted,
    ContextExhausted,
    OutputLimit,
    ModelUnavailable,
    AuthFailed,
    RunnerUnhealthy,
    UnknownProviderFailure,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_unbounded_values() {
        assert!(matches!(HarnessId::new(" "), Err(ValueError::Empty { .. })));
        assert!(matches!(
            ModelId::new("x".repeat(257)),
            Err(ValueError::TooLong { .. })
        ));
        assert!(ComplexityEstimate::new(0).is_err());
        assert!(DispatchPolicyVersion::new(0).is_err());
        assert!(ProjectMappingRevision::new(0).is_err());
    }

    #[test]
    fn serializes_stable_enum_names() {
        let encoded = serde_json::to_string(&ProviderCapacity::QuotaExhausted).unwrap();
        assert_eq!(encoded, "\"quota_exhausted\"");
        assert_eq!(
            serde_json::from_str::<ProviderCapacity>(&encoded).unwrap(),
            ProviderCapacity::QuotaExhausted
        );
    }

    #[test]
    fn deserialization_uses_value_object_validation() {
        assert!(serde_json::from_str::<HarnessId>("\"\"").is_err());
        assert!(serde_json::from_str::<ComplexityEstimate>("0").is_err());
        assert!(serde_json::from_str::<DispatchPolicyVersion>("0").is_err());
        assert!(serde_json::from_str::<ProjectMappingRevision>("0").is_err());
    }
}
