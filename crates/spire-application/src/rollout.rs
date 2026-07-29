//! Restricted-rollout gates and the operator kill switch.
//!
//! The gate guards admission only. Monitoring, recovery, reconciliation, and the
//! delivery of already-committed effects must keep running when the kill switch
//! is engaged, otherwise stopping admission would strand live work.

use std::collections::BTreeSet;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RolloutGate {
    pub linear_writes_enabled: bool,
    pub kill_switch_engaged: bool,
    pub allowed_team_ids: BTreeSet<String>,
    pub allowed_repositories: BTreeSet<String>,
    pub allowed_type_labels: BTreeSet<String>,
    pub max_active_harness_runs: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionCandidate<'a> {
    pub team_id: &'a str,
    pub repository: &'a str,
    pub labels: &'a BTreeSet<String>,
    pub active_harness_runs: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum RolloutDecision {
    Admit,
    Refuse {
        reason: RolloutRefusal,
        operator_detail: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutRefusal {
    AutomationDisabled,
    KillSwitchEngaged,
    TeamNotAllowlisted,
    RepositoryNotAllowlisted,
    WorkTypeNotAllowlisted,
    ActiveRunLimitReached,
}

impl RolloutGate {
    /// Whether committed external effects may still be delivered. This ignores
    /// the kill switch: an in-flight claim must converge rather than dangle.
    pub fn delivery_permitted(&self) -> bool {
        self.linear_writes_enabled
    }

    /// Whether new work may be admitted at all.
    pub fn admission_permitted(&self) -> bool {
        self.linear_writes_enabled && !self.kill_switch_engaged
    }
}

/// Evaluates every rollout dimension and always explains the refusal.
pub fn evaluate_rollout(gate: &RolloutGate, candidate: AdmissionCandidate<'_>) -> RolloutDecision {
    if !gate.linear_writes_enabled {
        return refuse(
            RolloutRefusal::AutomationDisabled,
            "rollout.linear_writes_enabled is false, so no ticket is admitted",
        );
    }
    if gate.kill_switch_engaged {
        return refuse(
            RolloutRefusal::KillSwitchEngaged,
            "the operator kill switch is engaged; monitoring and recovery continue",
        );
    }
    if !gate.allowed_team_ids.contains(candidate.team_id) {
        return refuse(
            RolloutRefusal::TeamNotAllowlisted,
            &format!("team {} is not in the pilot allowlist", candidate.team_id),
        );
    }
    if !gate.allowed_repositories.contains(candidate.repository) {
        return refuse(
            RolloutRefusal::RepositoryNotAllowlisted,
            &format!(
                "repository {} is not in the pilot allowlist",
                candidate.repository
            ),
        );
    }
    if !candidate
        .labels
        .iter()
        .any(|label| gate.allowed_type_labels.contains(label))
    {
        return refuse(
            RolloutRefusal::WorkTypeNotAllowlisted,
            "the ticket carries no allowlisted pilot work-type label",
        );
    }
    if candidate.active_harness_runs >= gate.max_active_harness_runs {
        return refuse(
            RolloutRefusal::ActiveRunLimitReached,
            &format!(
                "{} active harness run(s) already reach the rollout limit of {}",
                candidate.active_harness_runs, gate.max_active_harness_runs
            ),
        );
    }
    RolloutDecision::Admit
}

fn refuse(reason: RolloutRefusal, operator_detail: &str) -> RolloutDecision {
    RolloutDecision::Refuse {
        reason,
        operator_detail: operator_detail.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> RolloutGate {
        RolloutGate {
            linear_writes_enabled: true,
            kill_switch_engaged: false,
            allowed_team_ids: ["team".to_owned()].into_iter().collect(),
            allowed_repositories: ["owner/spire".to_owned()].into_iter().collect(),
            allowed_type_labels: ["type:chore".to_owned()].into_iter().collect(),
            max_active_harness_runs: 1,
        }
    }

    fn labels(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn candidate<'a>(labels: &'a BTreeSet<String>, active: u16) -> AdmissionCandidate<'a> {
        AdmissionCandidate {
            team_id: "team",
            repository: "owner/spire",
            labels,
            active_harness_runs: active,
        }
    }

    #[test]
    fn the_pilot_admits_only_one_team_repository_type_and_run() {
        let allowed = labels(&["type:chore"]);
        assert_eq!(
            evaluate_rollout(&gate(), candidate(&allowed, 0)),
            RolloutDecision::Admit
        );
        for (gate, candidate, expected) in [
            (
                gate(),
                AdmissionCandidate {
                    team_id: "other",
                    ..candidate(&allowed, 0)
                },
                RolloutRefusal::TeamNotAllowlisted,
            ),
            (
                gate(),
                AdmissionCandidate {
                    repository: "owner/other",
                    ..candidate(&allowed, 0)
                },
                RolloutRefusal::RepositoryNotAllowlisted,
            ),
            (
                gate(),
                candidate(&allowed, 1),
                RolloutRefusal::ActiveRunLimitReached,
            ),
            (
                RolloutGate {
                    linear_writes_enabled: false,
                    ..gate()
                },
                candidate(&allowed, 0),
                RolloutRefusal::AutomationDisabled,
            ),
            (
                RolloutGate {
                    kill_switch_engaged: true,
                    ..gate()
                },
                candidate(&allowed, 0),
                RolloutRefusal::KillSwitchEngaged,
            ),
        ] {
            let decision = evaluate_rollout(&gate, candidate);
            assert!(
                matches!(&decision, RolloutDecision::Refuse { reason, operator_detail }
                    if *reason == expected && !operator_detail.is_empty()),
                "expected {expected:?}, found {decision:?}"
            );
        }
        let unsupported = labels(&["type:bug"]);
        assert!(matches!(
            evaluate_rollout(&gate(), candidate(&unsupported, 0)),
            RolloutDecision::Refuse {
                reason: RolloutRefusal::WorkTypeNotAllowlisted,
                ..
            }
        ));
    }

    #[test]
    fn the_kill_switch_stops_admission_but_not_delivery() {
        let engaged = RolloutGate {
            kill_switch_engaged: true,
            ..gate()
        };
        assert!(!engaged.admission_permitted());
        assert!(engaged.delivery_permitted());
        let disabled = RolloutGate {
            linear_writes_enabled: false,
            ..gate()
        };
        assert!(!disabled.delivery_permitted());
    }
}
