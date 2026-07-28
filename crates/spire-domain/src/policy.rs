use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CandidateIndex, ComplexityClass, DispatchPolicyVersion, DispatchRuleId, Effort, HarnessId,
    ModelId, RunRole,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchCandidate {
    pub harness: HarnessId,
    pub model: ModelId,
    pub effort: Effort,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchRule {
    pub id: DispatchRuleId,
    pub role: RunRole,
    pub complexity: Vec<ComplexityClass>,
    pub candidates: Vec<DispatchCandidate>,
}

impl DispatchRule {
    fn matches(&self, role: RunRole, complexity: ComplexityClass) -> bool {
        self.role == role && self.complexity.contains(&complexity)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchPolicy {
    pub policy_version: DispatchPolicyVersion,
    pub rules: Vec<DispatchRule>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HarnessCapabilityRegistry {
    harnesses: BTreeMap<HarnessId, HarnessCapability>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HarnessCapability {
    models: BTreeSet<ModelId>,
    efforts: BTreeSet<Effort>,
}

impl HarnessCapabilityRegistry {
    pub fn register(
        &mut self,
        harness: HarnessId,
        models: impl IntoIterator<Item = ModelId>,
        efforts: impl IntoIterator<Item = Effort>,
    ) {
        self.harnesses.insert(
            harness,
            HarnessCapability {
                models: models.into_iter().collect(),
                efforts: efforts.into_iter().collect(),
            },
        );
    }

    pub fn supports(&self, candidate: &DispatchCandidate) -> bool {
        self.harnesses
            .get(&candidate.harness)
            .is_some_and(|capability| {
                capability.models.contains(&candidate.model)
                    && capability.efforts.contains(&candidate.effort)
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAvailability {
    Available,
    CircuitOpen,
    AuthDisabled,
    ModelUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderHealth {
    pub candidate: DispatchCandidate,
    pub availability: ProviderAvailability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSkipReason {
    UnsupportedCapability,
    CircuitOpen,
    AuthDisabled,
    ModelUnavailable,
    SameAsMaker,
    LowerPriority,
    Selected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateEvaluation {
    pub index: CandidateIndex,
    pub candidate: DispatchCandidate,
    pub reason: CandidateSkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DispatchEvaluation {
    pub policy_version: DispatchPolicyVersion,
    pub rule_id: DispatchRuleId,
    pub candidates: Vec<CandidateEvaluation>,
    pub selected: Option<CandidateIndex>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DispatchPolicyError {
    #[error("dispatch rule ID is duplicated: {0}")]
    DuplicateRuleId(DispatchRuleId),
    #[error("{role:?}/{complexity:?} matches {matches} rules; expected exactly one")]
    Coverage {
        role: RunRole,
        complexity: ComplexityClass,
        matches: usize,
    },
    #[error("dispatch rule {0} has no candidates")]
    NoCandidates(DispatchRuleId),
    #[error("candidate in rule {rule} is not supported by its harness registry")]
    UnsupportedCandidate { rule: DispatchRuleId },
    #[error(
        "implementation candidate {harness} has no possible different-harness reviewer for {complexity:?}"
    )]
    NoDifferentHarnessReviewer {
        harness: HarnessId,
        complexity: ComplexityClass,
    },
}

impl DispatchPolicy {
    pub fn validate(
        &self,
        registry: &HarnessCapabilityRegistry,
    ) -> Result<(), DispatchPolicyError> {
        let mut rule_ids = BTreeSet::new();
        for rule in &self.rules {
            if !rule_ids.insert(rule.id.clone()) {
                return Err(DispatchPolicyError::DuplicateRuleId(rule.id.clone()));
            }
            if rule.candidates.is_empty() {
                return Err(DispatchPolicyError::NoCandidates(rule.id.clone()));
            }
            if rule
                .candidates
                .iter()
                .any(|candidate| !registry.supports(candidate))
            {
                return Err(DispatchPolicyError::UnsupportedCandidate {
                    rule: rule.id.clone(),
                });
            }
        }

        for role in [RunRole::Implementation, RunRole::Review] {
            for complexity in ComplexityClass::ALL {
                let matches = self
                    .rules
                    .iter()
                    .filter(|rule| rule.matches(role, complexity))
                    .count();
                if matches != 1 {
                    return Err(DispatchPolicyError::Coverage {
                        role,
                        complexity,
                        matches,
                    });
                }
            }
        }

        for complexity in ComplexityClass::ALL {
            let implementation = self
                .matching_rule(RunRole::Implementation, complexity)
                .expect("validated coverage");
            let review = self
                .matching_rule(RunRole::Review, complexity)
                .expect("validated coverage");
            for candidate in &implementation.candidates {
                if !review
                    .candidates
                    .iter()
                    .any(|reviewer| reviewer.harness != candidate.harness)
                {
                    return Err(DispatchPolicyError::NoDifferentHarnessReviewer {
                        harness: candidate.harness.clone(),
                        complexity,
                    });
                }
            }
        }

        Ok(())
    }

    pub fn evaluate(
        &self,
        registry: &HarnessCapabilityRegistry,
        role: RunRole,
        complexity: ComplexityClass,
        health: &[ProviderHealth],
        sticky_maker: Option<&HarnessId>,
    ) -> Result<DispatchEvaluation, DispatchPolicyError> {
        self.validate(registry)?;
        let rule = self
            .matching_rule(role, complexity)
            .expect("validated coverage");
        let availability = health
            .iter()
            .map(|entry| (entry.candidate.clone(), entry.availability))
            .collect::<BTreeMap<_, _>>();
        let mut selected = None;
        let candidates = rule
            .candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                let reason = if selected.is_some() {
                    CandidateSkipReason::LowerPriority
                } else if !registry.supports(candidate) {
                    CandidateSkipReason::UnsupportedCapability
                } else if role == RunRole::Review
                    && sticky_maker.is_some_and(|maker| maker == &candidate.harness)
                {
                    CandidateSkipReason::SameAsMaker
                } else {
                    match availability
                        .get(candidate)
                        .copied()
                        .unwrap_or(ProviderAvailability::Available)
                    {
                        ProviderAvailability::Available => {
                            selected = Some(CandidateIndex::new(index));
                            CandidateSkipReason::Selected
                        }
                        ProviderAvailability::CircuitOpen => CandidateSkipReason::CircuitOpen,
                        ProviderAvailability::AuthDisabled => CandidateSkipReason::AuthDisabled,
                        ProviderAvailability::ModelUnavailable => {
                            CandidateSkipReason::ModelUnavailable
                        }
                    }
                };
                CandidateEvaluation {
                    index: CandidateIndex::new(index),
                    candidate: candidate.clone(),
                    reason,
                }
            })
            .collect();

        Ok(DispatchEvaluation {
            policy_version: self.policy_version,
            rule_id: rule.id.clone(),
            candidates,
            selected,
        })
    }

    fn matching_rule(&self, role: RunRole, complexity: ComplexityClass) -> Option<&DispatchRule> {
        self.rules
            .iter()
            .find(|rule| rule.matches(role, complexity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> HarnessId {
        HarnessId::new(value).unwrap()
    }

    fn model(value: &str) -> ModelId {
        ModelId::new(value).unwrap()
    }

    fn candidate(harness: &str) -> DispatchCandidate {
        DispatchCandidate {
            harness: id(harness),
            model: model(&format!("{harness}-model")),
            effort: Effort::Medium,
        }
    }

    fn registry() -> HarnessCapabilityRegistry {
        let mut registry = HarnessCapabilityRegistry::default();
        for harness in ["codex", "claude-code"] {
            registry.register(
                id(harness),
                [model(&format!("{harness}-model"))],
                [Effort::Medium],
            );
        }
        registry
    }

    fn policy() -> DispatchPolicy {
        DispatchPolicy {
            policy_version: DispatchPolicyVersion::new(1).unwrap(),
            rules: [RunRole::Implementation, RunRole::Review]
                .into_iter()
                .map(|role| DispatchRule {
                    id: DispatchRuleId::new(if role == RunRole::Implementation {
                        "implementation"
                    } else {
                        "review"
                    })
                    .unwrap(),
                    role,
                    complexity: ComplexityClass::ALL.to_vec(),
                    candidates: if role == RunRole::Implementation {
                        vec![candidate("codex"), candidate("claude-code")]
                    } else {
                        vec![candidate("claude-code"), candidate("codex")]
                    },
                })
                .collect(),
        }
    }

    #[test]
    fn policy_has_exact_coverage_and_different_harness_reviewers() {
        assert!(policy().validate(&registry()).is_ok());
    }

    #[test]
    fn overlapping_rules_are_rejected() {
        let mut policy = policy();
        policy.rules.push(policy.rules[0].clone());
        assert!(matches!(
            policy.validate(&registry()),
            Err(DispatchPolicyError::DuplicateRuleId(_))
        ));
    }

    #[test]
    fn review_never_selects_the_sticky_maker() {
        let policy = policy();
        let evaluation = policy
            .evaluate(
                &registry(),
                RunRole::Review,
                ComplexityClass::Small,
                &[],
                Some(&id("claude-code")),
            )
            .unwrap();
        assert_eq!(evaluation.selected, Some(CandidateIndex::new(1)));
        assert_eq!(
            evaluation.candidates[0].reason,
            CandidateSkipReason::SameAsMaker
        );
    }

    #[test]
    fn no_healthy_candidate_returns_a_wait_evaluation() {
        let policy = policy();
        let health = policy.rules[0]
            .candidates
            .iter()
            .cloned()
            .map(|candidate| ProviderHealth {
                candidate,
                availability: ProviderAvailability::CircuitOpen,
            })
            .collect::<Vec<_>>();
        let evaluation = policy
            .evaluate(
                &registry(),
                RunRole::Implementation,
                ComplexityClass::Small,
                &health,
                None,
            )
            .unwrap();
        assert_eq!(evaluation.selected, None);
    }
}
