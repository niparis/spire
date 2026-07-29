use std::cmp::Ordering;

use serde::Serialize;
use spire_domain::{
    CandidateEvaluation, ComplexityClass, DispatchCandidate, ProviderAvailability, ProviderHealth,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EligibleWork {
    pub work_item_id: String,
    pub linear_identifier: String,
    pub repository: String,
    pub manual_expedite: bool,
    pub priority: Option<u8>,
    pub ready_at: i64,
    pub complexity: ComplexityClass,
    pub initiator: SchedulerInitiator,
}

impl Ord for EligibleWork {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .manual_expedite
            .cmp(&self.manual_expedite)
            .then_with(|| other.priority.cmp(&self.priority))
            .then_with(|| self.ready_at.cmp(&other.ready_at))
            .then_with(|| self.linear_identifier.cmp(&other.linear_identifier))
    }
}
impl PartialOrd for EligibleWork {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedulerInitiator {
    Human,
    Ai,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CapacityLimits {
    pub total: u16,
    pub ai: u16,
    pub per_repository: u16,
    pub per_ticket: u16,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct CapacityCounts {
    pub total: u16,
    pub ai: u16,
    pub repository: u16,
    pub ticket: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimBlock {
    TotalCapacity,
    AiCapacity,
    RepositoryCapacity,
    TicketCapacity,
}

pub const MAX_INITIAL_CI_CORRECTION_CYCLES: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CiCorrectionDecision {
    Dispatch { next_cycle: u8 },
    WaitForCapacity(ClaimBlock),
    Exhausted,
}

/// Capacity waits are operational, not engineering failures: they never consume
/// one of the two bounded CI correction cycles.
pub fn plan_ci_correction(
    completed_cycles: u8,
    limits: CapacityLimits,
    counts: CapacityCounts,
) -> CiCorrectionDecision {
    if completed_cycles >= MAX_INITIAL_CI_CORRECTION_CYCLES {
        return CiCorrectionDecision::Exhausted;
    }
    match capacity_allows(limits, counts, SchedulerInitiator::Ai) {
        Ok(()) => CiCorrectionDecision::Dispatch {
            next_cycle: completed_cycles + 1,
        },
        Err(block) => CiCorrectionDecision::WaitForCapacity(block),
    }
}

pub fn capacity_allows(
    limits: CapacityLimits,
    counts: CapacityCounts,
    initiator: SchedulerInitiator,
) -> Result<(), ClaimBlock> {
    if counts.total >= limits.total {
        return Err(ClaimBlock::TotalCapacity);
    }
    if initiator == SchedulerInitiator::Ai && counts.ai >= limits.ai {
        return Err(ClaimBlock::AiCapacity);
    }
    if counts.repository >= limits.per_repository {
        return Err(ClaimBlock::RepositoryCapacity);
    }
    if counts.ticket >= limits.per_ticket {
        return Err(ClaimBlock::TicketCapacity);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSelection {
    Selected {
        candidate: DispatchCandidate,
        evaluations: Vec<CandidateEvaluation>,
    },
    WaitingForProvider {
        evaluations: Vec<CandidateEvaluation>,
        retry_at: Option<i64>,
    },
}

pub fn select_healthy_candidate(
    candidates: &[DispatchCandidate],
    health: &[ProviderHealth],
    retry_at: Option<i64>,
) -> CandidateSelection {
    let mut selected = None;
    let evaluations = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let availability = health
                .iter()
                .find(|entry| entry.candidate == *candidate)
                .map(|entry| entry.availability)
                .unwrap_or(ProviderAvailability::Available);
            let reason = if selected.is_some() {
                spire_domain::CandidateSkipReason::LowerPriority
            } else if availability == ProviderAvailability::Available {
                selected = Some(candidate.clone());
                spire_domain::CandidateSkipReason::Selected
            } else {
                match availability {
                    ProviderAvailability::CircuitOpen => {
                        spire_domain::CandidateSkipReason::CircuitOpen
                    }
                    ProviderAvailability::AuthDisabled => {
                        spire_domain::CandidateSkipReason::AuthDisabled
                    }
                    ProviderAvailability::ModelUnavailable => {
                        spire_domain::CandidateSkipReason::ModelUnavailable
                    }
                    ProviderAvailability::Available => unreachable!(),
                }
            };
            CandidateEvaluation {
                index: spire_domain::CandidateIndex::new(index),
                candidate: candidate.clone(),
                reason,
            }
        })
        .collect::<Vec<_>>();
    match selected {
        Some(candidate) => CandidateSelection::Selected {
            candidate,
            evaluations,
        },
        None => CandidateSelection::WaitingForProvider {
            evaluations,
            retry_at,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DispatchPlan {
    pub work_item_id: String,
    pub revision: String,
    pub raw_estimate: u8,
    pub complexity: ComplexityClass,
    pub implementation_rule_id: String,
    pub review_rule_id: String,
    pub selected: DispatchCandidate,
    pub evaluations: Vec<CandidateEvaluation>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use spire_domain::{Effort, HarnessId, ModelId};
    fn work(id: &str, expedite: bool, priority: Option<u8>, ready_at: i64) -> EligibleWork {
        EligibleWork {
            work_item_id: id.into(),
            linear_identifier: id.into(),
            repository: "owner/repo".into(),
            manual_expedite: expedite,
            priority,
            ready_at,
            complexity: ComplexityClass::Small,
            initiator: SchedulerInitiator::Human,
        }
    }
    fn candidate(harness: &str) -> DispatchCandidate {
        DispatchCandidate {
            harness: HarnessId::new(harness).unwrap(),
            model: ModelId::new("model").unwrap(),
            effort: Effort::Medium,
        }
    }
    #[test]
    fn ordering_is_deterministic_and_ignores_complexity() {
        let mut queue = vec![
            work("B", false, Some(1), 3),
            work("A", true, None, 9),
            work("C", false, Some(2), 1),
        ];
        queue.sort();
        assert_eq!(
            queue
                .into_iter()
                .map(|item| item.linear_identifier)
                .collect::<Vec<_>>(),
            ["A", "C", "B"]
        );
    }
    #[test]
    fn ai_capacity_does_not_block_human_work_but_total_still_applies() {
        let limits = CapacityLimits {
            total: 3,
            ai: 1,
            per_repository: 3,
            per_ticket: 1,
        };
        assert_eq!(
            capacity_allows(
                limits,
                CapacityCounts {
                    total: 1,
                    ai: 1,
                    repository: 1,
                    ticket: 0
                },
                SchedulerInitiator::Ai
            ),
            Err(ClaimBlock::AiCapacity)
        );
        assert!(
            capacity_allows(
                limits,
                CapacityCounts {
                    total: 1,
                    ai: 1,
                    repository: 1,
                    ticket: 0
                },
                SchedulerInitiator::Human
            )
            .is_ok()
        );
        assert_eq!(
            capacity_allows(
                limits,
                CapacityCounts {
                    total: 3,
                    ai: 1,
                    repository: 1,
                    ticket: 0
                },
                SchedulerInitiator::Human
            ),
            Err(ClaimBlock::TotalCapacity)
        );
    }
    #[test]
    fn provider_fallback_preserves_all_evaluations() {
        let first = candidate("first");
        let second = candidate("second");
        let selection = select_healthy_candidate(
            &[first.clone(), second.clone()],
            &[ProviderHealth {
                candidate: first,
                availability: ProviderAvailability::CircuitOpen,
            }],
            Some(42),
        );
        assert!(
            matches!(selection, CandidateSelection::Selected { candidate, evaluations } if candidate == second && evaluations.len() == 2)
        );
    }

    #[test]
    fn ci_capacity_wait_does_not_consume_an_engineering_cycle() {
        let limits = CapacityLimits {
            total: 3,
            ai: 1,
            per_repository: 1,
            per_ticket: 1,
        };
        assert_eq!(
            plan_ci_correction(
                1,
                limits,
                CapacityCounts {
                    total: 1,
                    ai: 1,
                    repository: 0,
                    ticket: 0
                }
            ),
            CiCorrectionDecision::WaitForCapacity(ClaimBlock::AiCapacity)
        );
        assert_eq!(
            plan_ci_correction(2, limits, CapacityCounts::default()),
            CiCorrectionDecision::Exhausted
        );
    }
}
