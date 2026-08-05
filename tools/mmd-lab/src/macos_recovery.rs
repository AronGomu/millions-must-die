//! macOS recovery protocol state machine (external controller only).
//!
//! Candidate OS never approves cleanup. Physical EACS/ADE/MDM/DFU drill is
//! outside this module; unit tests exercise protocol transitions only.
//! No generic multi-provider MDM adapter — single frozen profile pin.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::macos::{MacosAttestResult, MacosAttestVerdict};

/// Network segments the external controller moves the host through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MacosLabNetwork {
    /// EACS / ADE / MDM / DFU plane. Apple/APNs/MDM allowlist only.
    Recovery,
    /// Post-wipe enrollment finish + identity freeze. No general internet.
    Provisioning,
    /// Candidate run plane. External controls must deny egress.
    Candidate,
}

impl fmt::Display for MacosLabNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recovery => write!(f, "recovery"),
            Self::Provisioning => write!(f, "provisioning"),
            Self::Candidate => write!(f, "candidate"),
        }
    }
}

/// Protocol phase. Terminal success = `ReadyForCandidate`. Drift = `Quarantined`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MacosRecoveryPhase {
    /// No active restore. Awaiting external controller.
    Idle,
    /// Controller owns power/network; EACS path selected.
    ExternalRecoveryControl,
    /// EACS wipe preflight against Apple/MDM path.
    EacsPreflight,
    /// Erase All Content and Settings in flight; awaiting reset ack.
    EacsWipe,
    /// ADE/MDM re-enrollment after wipe (or after DFU).
    ReenrollingMdm,
    /// Minting fresh host identity; old identity discarded.
    RotatingHostIdentity,
    /// Moving host across recovery → provisioning → candidate VLANs.
    NetworkTransition {
        from: MacosLabNetwork,
        to: MacosLabNetwork,
    },
    /// Host attestation against frozen macOS runner contract.
    Attesting,
    /// External egress canary from candidate VLAN must be denied.
    EgressCanary,
    /// Host ready for candidate archive dispatch (T23+).
    ReadyForCandidate,
    /// Manual DFU restore via second Mac + USB-C (EACS fallback).
    DfuFallback,
    /// Drift / missed EACS ack / reenroll fail / SSV / egress. No candidate.
    Quarantined { reason: String },
}

impl MacosRecoveryPhase {
    pub fn is_terminal_success(&self) -> bool {
        matches!(self, Self::ReadyForCandidate)
    }

    pub fn is_quarantined(&self) -> bool {
        matches!(self, Self::Quarantined { .. })
    }

    /// Candidate provision / archive dispatch allowed only in ReadyForCandidate.
    pub fn allows_candidate_provision(&self) -> bool {
        matches!(self, Self::ReadyForCandidate)
    }
}

/// External controller / operator observations. Never supplied by candidate OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacosRecoveryEvent {
    /// Controller starts EACS/ADE restore; candidate must not trigger this.
    StartExternalRestore {
        /// True when start originates from external controller (required).
        external_controller: bool,
    },
    /// EACS preflight result from recovery plane.
    EacsPreflightFinished {
        ok: bool,
        detail: String,
    },
    /// EACS wipe finished; ack must arrive or path quarantines.
    EacsWipeFinished {
        /// False on timeout / missing reset ack → quarantine.
        ack_received: bool,
        detail: String,
    },
    /// ADE/MDM re-enrollment result after wipe or DFU.
    MdmReenrollFinished {
        success: bool,
        /// Observed profile id (must match frozen pin when success).
        profile_id: String,
        detail: String,
    },
    /// Fresh macOS host identity observed after rotate.
    HostIdentityRotated {
        new_host_identity: String,
        /// Identity seen before rotate (must not equal new).
        prior_host_identity: Option<String>,
    },
    /// Controller completed a network move.
    NetworkMoved { network: MacosLabNetwork },
    /// Result of host attestation (coordinator-side, not candidate).
    AttestationFinished(MacosAttestResult),
    /// External egress canary result on candidate VLAN.
    EgressCanaryFinished {
        /// True when probe was denied by external controls (required pass).
        denied: bool,
        detail: String,
    },
    /// Operator starts DFU fallback (second Mac + cable). Clears sticky quarantine.
    StartDfuFallback {
        /// True when human operator on recovery bench initiates DFU (required).
        external_operator: bool,
    },
    /// DFU restore finished on recovery bench.
    DfuRestoreFinished {
        success: bool,
        detail: String,
    },
    /// Explicit re-quarantine (manual, drift watch, or failed post-run EACS).
    ForceQuarantine { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacosRecoveryState {
    pub phase: MacosRecoveryPhase,
    /// Frozen MDM profile id pin (from runner manifest / mdm-profile example).
    pub expected_mdm_profile_id: String,
    /// True after successful EACS wipe ack or DFU restore.
    pub reset_completed: bool,
    /// True after successful ADE/MDM reenroll against expected profile.
    pub mdm_reenrolled: bool,
    /// Recovery path used for last successful reset (`eacs` or `dfu`).
    pub reset_path: Option<String>,
    /// Current host identity after rotate.
    pub host_identity: Option<String>,
    /// Identity that must not reappear after rotate.
    pub retired_host_identity: Option<String>,
    /// Network the controller last confirmed.
    pub network: Option<MacosLabNetwork>,
    /// Ordered log of phase names for drill evidence.
    pub trail: Vec<String>,
    /// Quarantine sticky until external restore or DFU clears it.
    pub quarantine_reason: Option<String>,
}

impl MacosRecoveryState {
    pub fn new(expected_mdm_profile_id: impl Into<String>) -> Self {
        Self {
            phase: MacosRecoveryPhase::Idle,
            expected_mdm_profile_id: expected_mdm_profile_id.into(),
            reset_completed: false,
            mdm_reenrolled: false,
            reset_path: None,
            host_identity: None,
            retired_host_identity: None,
            network: None,
            trail: vec!["idle".into()],
            quarantine_reason: None,
        }
    }

    fn quarantine(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        self.quarantine_reason = Some(reason.clone());
        self.phase = MacosRecoveryPhase::Quarantined {
            reason: reason.clone(),
        };
        self.trail.push(format!("quarantined:{reason}"));
        // Clear ready markers so provision cannot proceed on sticky quarantine.
        self.reset_completed = false;
        self.mdm_reenrolled = false;
        self.reset_path = None;
    }

    fn enter(&mut self, phase: MacosRecoveryPhase) {
        let label = match &phase {
            MacosRecoveryPhase::Idle => "idle".into(),
            MacosRecoveryPhase::ExternalRecoveryControl => "external-recovery-control".into(),
            MacosRecoveryPhase::EacsPreflight => "eacs-preflight".into(),
            MacosRecoveryPhase::EacsWipe => "eacs-wipe".into(),
            MacosRecoveryPhase::ReenrollingMdm => "reenrolling-mdm".into(),
            MacosRecoveryPhase::RotatingHostIdentity => "rotating-host-identity".into(),
            MacosRecoveryPhase::NetworkTransition { from, to } => {
                format!("network-transition:{from}->{to}")
            }
            MacosRecoveryPhase::Attesting => "attesting".into(),
            MacosRecoveryPhase::EgressCanary => "egress-canary".into(),
            MacosRecoveryPhase::ReadyForCandidate => "ready-for-candidate".into(),
            MacosRecoveryPhase::DfuFallback => "dfu-fallback".into(),
            MacosRecoveryPhase::Quarantined { reason } => format!("quarantined:{reason}"),
        };
        self.trail.push(label);
        self.phase = phase;
    }

    fn clear_session_markers(&mut self) {
        self.reset_completed = false;
        self.mdm_reenrolled = false;
        self.reset_path = None;
        self.host_identity = None;
        self.retired_host_identity = None;
        self.network = None;
        self.quarantine_reason = None;
    }

    /// Apply one external-controller event. Returns error string only for
    /// programmer misuse (event in wrong phase); policy failures quarantine.
    pub fn apply(&mut self, event: MacosRecoveryEvent) -> Result<(), String> {
        // Sticky quarantine: only fresh external EACS restore or DFU may leave it.
        if let MacosRecoveryPhase::Quarantined { .. } = &self.phase {
            match &event {
                MacosRecoveryEvent::StartExternalRestore {
                    external_controller: true,
                } => {
                    self.clear_session_markers();
                    self.enter(MacosRecoveryPhase::ExternalRecoveryControl);
                    return Ok(());
                }
                MacosRecoveryEvent::StartDfuFallback {
                    external_operator: true,
                } => {
                    self.clear_session_markers();
                    self.enter(MacosRecoveryPhase::DfuFallback);
                    return Ok(());
                }
                MacosRecoveryEvent::ForceQuarantine { reason } => {
                    self.quarantine(reason.clone());
                    return Ok(());
                }
                _ => {
                    self.trail
                        .push("quarantine-persistence:ignored-non-restore-event".into());
                    return Ok(());
                }
            }
        }

        match event {
            MacosRecoveryEvent::StartExternalRestore {
                external_controller,
            } => {
                if !external_controller {
                    self.quarantine("candidate-initiated restore rejected");
                    return Ok(());
                }
                match self.phase {
                    MacosRecoveryPhase::Idle
                    | MacosRecoveryPhase::ReadyForCandidate
                    | MacosRecoveryPhase::ExternalRecoveryControl => {
                        self.clear_session_markers();
                        self.enter(MacosRecoveryPhase::ExternalRecoveryControl);
                        Ok(())
                    }
                    ref other => Err(format!(
                        "StartExternalRestore invalid in phase {other:?}"
                    )),
                }
            }

            MacosRecoveryEvent::EacsPreflightFinished { ok, detail } => {
                if self.phase != MacosRecoveryPhase::ExternalRecoveryControl {
                    return Err(format!(
                        "EacsPreflightFinished invalid in phase {:?}",
                        self.phase
                    ));
                }
                self.network = Some(MacosLabNetwork::Recovery);
                self.enter(MacosRecoveryPhase::EacsPreflight);
                if !ok {
                    self.quarantine(format!("eacs preflight failed: {detail}"));
                    return Ok(());
                }
                self.enter(MacosRecoveryPhase::EacsWipe);
                Ok(())
            }

            MacosRecoveryEvent::EacsWipeFinished {
                ack_received,
                detail,
            } => {
                if self.phase != MacosRecoveryPhase::EacsWipe {
                    return Err(format!(
                        "EacsWipeFinished invalid in phase {:?}",
                        self.phase
                    ));
                }
                if !ack_received {
                    self.quarantine(format!("missed eacs ack: {detail}"));
                    return Ok(());
                }
                self.reset_completed = true;
                self.reset_path = Some("eacs".into());
                self.trail.push(format!("eacs-ack:{detail}"));
                self.enter(MacosRecoveryPhase::ReenrollingMdm);
                Ok(())
            }

            MacosRecoveryEvent::MdmReenrollFinished {
                success,
                profile_id,
                detail,
            } => {
                if self.phase != MacosRecoveryPhase::ReenrollingMdm {
                    return Err(format!(
                        "MdmReenrollFinished invalid in phase {:?}",
                        self.phase
                    ));
                }
                if !success {
                    self.quarantine(format!("reenroll failure: {detail}"));
                    return Ok(());
                }
                if profile_id != self.expected_mdm_profile_id {
                    self.quarantine(format!(
                        "reenroll failure: profile_id {profile_id} != expected {}",
                        self.expected_mdm_profile_id
                    ));
                    return Ok(());
                }
                if !self.reset_completed {
                    self.quarantine("reenroll without completed reset");
                    return Ok(());
                }
                self.mdm_reenrolled = true;
                self.trail.push(format!("mdm-reenrolled:{profile_id}"));
                self.enter(MacosRecoveryPhase::RotatingHostIdentity);
                Ok(())
            }

            MacosRecoveryEvent::HostIdentityRotated {
                new_host_identity,
                prior_host_identity,
            } => {
                if self.phase != MacosRecoveryPhase::RotatingHostIdentity {
                    return Err(format!(
                        "HostIdentityRotated invalid in phase {:?}",
                        self.phase
                    ));
                }
                if new_host_identity.trim().is_empty() {
                    self.quarantine("empty host identity after rotate");
                    return Ok(());
                }
                if let Some(prior) = prior_host_identity.as_ref() {
                    if prior == &new_host_identity {
                        self.quarantine(
                            "stale macos identity: fingerprint unchanged after rotate",
                        );
                        return Ok(());
                    }
                    self.retired_host_identity = Some(prior.clone());
                }
                if self.retired_host_identity.as_ref() == Some(&new_host_identity) {
                    self.quarantine("stale macos identity: retired fingerprint reused");
                    return Ok(());
                }
                self.host_identity = Some(new_host_identity);
                self.enter(MacosRecoveryPhase::NetworkTransition {
                    from: MacosLabNetwork::Recovery,
                    to: MacosLabNetwork::Provisioning,
                });
                Ok(())
            }

            MacosRecoveryEvent::NetworkMoved { network } => match &self.phase {
                MacosRecoveryPhase::NetworkTransition { from, to } => {
                    if network != *to {
                        self.quarantine(format!(
                            "network move to {network} != expected {to} (from {from})"
                        ));
                        return Ok(());
                    }
                    self.network = Some(network);
                    match network {
                        MacosLabNetwork::Provisioning => {
                            self.enter(MacosRecoveryPhase::NetworkTransition {
                                from: MacosLabNetwork::Provisioning,
                                to: MacosLabNetwork::Candidate,
                            });
                        }
                        MacosLabNetwork::Candidate => {
                            self.enter(MacosRecoveryPhase::Attesting);
                        }
                        MacosLabNetwork::Recovery => {
                            self.quarantine("unexpected return to recovery mid-transition");
                        }
                    }
                    Ok(())
                }
                other => Err(format!("NetworkMoved invalid in phase {other:?}")),
            },

            MacosRecoveryEvent::AttestationFinished(result) => {
                if self.phase != MacosRecoveryPhase::Attesting {
                    return Err(format!(
                        "AttestationFinished invalid in phase {:?}",
                        self.phase
                    ));
                }
                match result.verdict {
                    MacosAttestVerdict::ReadyForRecovery => {
                        self.enter(MacosRecoveryPhase::EgressCanary);
                        Ok(())
                    }
                    MacosAttestVerdict::Quarantine
                    | MacosAttestVerdict::Reject
                    | MacosAttestVerdict::MaintenanceBlock => {
                        let why = if result.reasons.is_empty() {
                            format!("attestation {:?}", result.verdict)
                        } else {
                            result.reasons.join("; ")
                        };
                        self.quarantine(why);
                        Ok(())
                    }
                }
            }

            MacosRecoveryEvent::EgressCanaryFinished { denied, detail } => {
                if self.phase != MacosRecoveryPhase::EgressCanary {
                    return Err(format!(
                        "EgressCanaryFinished invalid in phase {:?}",
                        self.phase
                    ));
                }
                if !denied {
                    self.quarantine(format!("egress canary not denied: {detail}"));
                    return Ok(());
                }
                if self.network != Some(MacosLabNetwork::Candidate) {
                    self.quarantine("egress canary without candidate network");
                    return Ok(());
                }
                if !self.reset_completed
                    || !self.mdm_reenrolled
                    || self.host_identity.is_none()
                {
                    self.quarantine("egress canary missing reset, mdm, or host identity");
                    return Ok(());
                }
                self.trail
                    .push(format!("egress-canary-denied:{detail}"));
                self.enter(MacosRecoveryPhase::ReadyForCandidate);
                Ok(())
            }

            MacosRecoveryEvent::StartDfuFallback {
                external_operator,
            } => {
                if !external_operator {
                    self.quarantine("candidate-initiated dfu rejected");
                    return Ok(());
                }
                match self.phase {
                    MacosRecoveryPhase::Idle
                    | MacosRecoveryPhase::ReadyForCandidate
                    | MacosRecoveryPhase::ExternalRecoveryControl
                    | MacosRecoveryPhase::EacsWipe
                    | MacosRecoveryPhase::DfuFallback => {
                        // DFU may interrupt failed/idle EACS path without sticky quarantine.
                        self.clear_session_markers();
                        self.enter(MacosRecoveryPhase::DfuFallback);
                        Ok(())
                    }
                    ref other => Err(format!(
                        "StartDfuFallback invalid in phase {other:?}"
                    )),
                }
            }

            MacosRecoveryEvent::DfuRestoreFinished { success, detail } => {
                if self.phase != MacosRecoveryPhase::DfuFallback {
                    return Err(format!(
                        "DfuRestoreFinished invalid in phase {:?}",
                        self.phase
                    ));
                }
                if !success {
                    self.quarantine(format!("dfu restore failure: {detail}"));
                    return Ok(());
                }
                self.network = Some(MacosLabNetwork::Recovery);
                self.reset_completed = true;
                self.reset_path = Some("dfu".into());
                self.trail.push(format!("dfu-restore-ok:{detail}"));
                self.enter(MacosRecoveryPhase::ReenrollingMdm);
                Ok(())
            }

            MacosRecoveryEvent::ForceQuarantine { reason } => {
                self.quarantine(reason);
                Ok(())
            }
        }
    }
}

/// Canonical happy-path EACS event sequence for unit/simulation drills and
/// the fixture candidate lane (T23).
pub fn simulate_successful_macos_eacs_drill(
    expected_mdm_profile_id: &str,
    new_host_identity: &str,
    prior_host_identity: Option<&str>,
) -> MacosRecoveryState {
    let mut s = MacosRecoveryState::new(expected_mdm_profile_id);
    let steps = [
        MacosRecoveryEvent::StartExternalRestore {
            external_controller: true,
        },
        MacosRecoveryEvent::EacsPreflightFinished {
            ok: true,
            detail: "eacs-preflight-ok".into(),
        },
        MacosRecoveryEvent::EacsWipeFinished {
            ack_received: true,
            detail: "eacs-reset-ack".into(),
        },
        MacosRecoveryEvent::MdmReenrollFinished {
            success: true,
            profile_id: expected_mdm_profile_id.to_string(),
            detail: "ade-mdm-ok".into(),
        },
        MacosRecoveryEvent::HostIdentityRotated {
            new_host_identity: new_host_identity.to_string(),
            prior_host_identity: prior_host_identity.map(str::to_string),
        },
        MacosRecoveryEvent::NetworkMoved {
            network: MacosLabNetwork::Provisioning,
        },
        MacosRecoveryEvent::NetworkMoved {
            network: MacosLabNetwork::Candidate,
        },
        MacosRecoveryEvent::AttestationFinished(MacosAttestResult::ok()),
        MacosRecoveryEvent::EgressCanaryFinished {
            denied: true,
            detail: "external-flow-log:tcp/443+dns blocked".into(),
        },
    ];
    for ev in steps {
        s.apply(ev).expect("happy path eacs event");
    }
    s
}

/// Canonical happy-path DFU fallback after missed EACS ack.
#[cfg_attr(not(test), allow(dead_code))]
pub fn simulate_successful_macos_dfu_drill(
    expected_mdm_profile_id: &str,
    new_host_identity: &str,
    prior_host_identity: Option<&str>,
) -> MacosRecoveryState {
    let mut s = MacosRecoveryState::new(expected_mdm_profile_id);
    s.apply(MacosRecoveryEvent::StartExternalRestore {
        external_controller: true,
    })
    .unwrap();
    s.apply(MacosRecoveryEvent::EacsPreflightFinished {
        ok: true,
        detail: "eacs-preflight-ok".into(),
    })
    .unwrap();
    s.apply(MacosRecoveryEvent::EacsWipeFinished {
        ack_received: false,
        detail: "timeout waiting reset ack".into(),
    })
    .unwrap();
    assert!(s.phase.is_quarantined());

    s.apply(MacosRecoveryEvent::StartDfuFallback {
        external_operator: true,
    })
    .unwrap();
    s.apply(MacosRecoveryEvent::DfuRestoreFinished {
        success: true,
        detail: "dfu-via-second-mac".into(),
    })
    .unwrap();

    let steps = [
        MacosRecoveryEvent::MdmReenrollFinished {
            success: true,
            profile_id: expected_mdm_profile_id.to_string(),
            detail: "ade-mdm-ok-after-dfu".into(),
        },
        MacosRecoveryEvent::HostIdentityRotated {
            new_host_identity: new_host_identity.to_string(),
            prior_host_identity: prior_host_identity.map(str::to_string),
        },
        MacosRecoveryEvent::NetworkMoved {
            network: MacosLabNetwork::Provisioning,
        },
        MacosRecoveryEvent::NetworkMoved {
            network: MacosLabNetwork::Candidate,
        },
        MacosRecoveryEvent::AttestationFinished(MacosAttestResult::ok()),
        MacosRecoveryEvent::EgressCanaryFinished {
            denied: true,
            detail: "external-flow-log:tcp/443+dns blocked".into(),
        },
    ];
    for ev in steps {
        s.apply(ev).expect("happy path dfu event");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos::MacosAttestResult;

    const PROFILE: &str = "mmd-lab-macos-ref";

    fn start_through_preflight() -> MacosRecoveryState {
        let mut s = MacosRecoveryState::new(PROFILE);
        s.apply(MacosRecoveryEvent::StartExternalRestore {
            external_controller: true,
        })
        .unwrap();
        s.apply(MacosRecoveryEvent::EacsPreflightFinished {
            ok: true,
            detail: "ok".into(),
        })
        .unwrap();
        s
    }

    #[test]
    fn missed_eacs_ack_quarantines() {
        let mut s = start_through_preflight();
        assert!(matches!(s.phase, MacosRecoveryPhase::EacsWipe));
        s.apply(MacosRecoveryEvent::EacsWipeFinished {
            ack_received: false,
            detail: "timeout".into(),
        })
        .unwrap();
        assert!(s.phase.is_quarantined(), "{:?}", s.phase);
        assert!(!s.phase.allows_candidate_provision());
        let reason = s.quarantine_reason.as_deref().unwrap_or("");
        assert!(
            reason.contains("missed eacs ack"),
            "reason={reason}"
        );
    }

    #[test]
    fn reenroll_failure_quarantines() {
        let mut s = start_through_preflight();
        s.apply(MacosRecoveryEvent::EacsWipeFinished {
            ack_received: true,
            detail: "ack".into(),
        })
        .unwrap();
        assert!(matches!(s.phase, MacosRecoveryPhase::ReenrollingMdm));
        s.apply(MacosRecoveryEvent::MdmReenrollFinished {
            success: false,
            profile_id: PROFILE.into(),
            detail: "mdm provider rejected enrollment".into(),
        })
        .unwrap();
        assert!(s.phase.is_quarantined(), "{:?}", s.phase);
        assert!(!s.phase.allows_candidate_provision());
        let reason = s.quarantine_reason.as_deref().unwrap_or("");
        assert!(
            reason.contains("reenroll failure"),
            "reason={reason}"
        );
    }

    #[test]
    fn reenroll_wrong_profile_quarantines() {
        let mut s = start_through_preflight();
        s.apply(MacosRecoveryEvent::EacsWipeFinished {
            ack_received: true,
            detail: "ack".into(),
        })
        .unwrap();
        s.apply(MacosRecoveryEvent::MdmReenrollFinished {
            success: true,
            profile_id: "wrong-profile".into(),
            detail: "ok-but-wrong-id".into(),
        })
        .unwrap();
        assert!(s.phase.is_quarantined());
        assert!(!s.phase.allows_candidate_provision());
    }

    #[test]
    fn mac_candidate_egress_blocks() {
        let mut s = MacosRecoveryState::new(PROFILE);
        for ev in [
            MacosRecoveryEvent::StartExternalRestore {
                external_controller: true,
            },
            MacosRecoveryEvent::EacsPreflightFinished {
                ok: true,
                detail: "ok".into(),
            },
            MacosRecoveryEvent::EacsWipeFinished {
                ack_received: true,
                detail: "ack".into(),
            },
            MacosRecoveryEvent::MdmReenrollFinished {
                success: true,
                profile_id: PROFILE.into(),
                detail: "ok".into(),
            },
            MacosRecoveryEvent::HostIdentityRotated {
                new_host_identity: "MAC-HOST-NEW".into(),
                prior_host_identity: Some("MAC-HOST-OLD".into()),
            },
            MacosRecoveryEvent::NetworkMoved {
                network: MacosLabNetwork::Provisioning,
            },
            MacosRecoveryEvent::NetworkMoved {
                network: MacosLabNetwork::Candidate,
            },
            MacosRecoveryEvent::AttestationFinished(MacosAttestResult::ok()),
        ] {
            s.apply(ev).unwrap();
        }
        assert!(matches!(s.phase, MacosRecoveryPhase::EgressCanary));
        s.apply(MacosRecoveryEvent::EgressCanaryFinished {
            denied: false,
            detail: "canary reached 1.1.1.1:443".into(),
        })
        .unwrap();
        assert!(s.phase.is_quarantined(), "{:?}", s.phase);
        assert!(!s.phase.allows_candidate_provision());
        let reason = s.quarantine_reason.as_deref().unwrap_or("");
        assert!(
            reason.contains("egress canary"),
            "reason={reason}"
        );
    }

    #[test]
    fn stale_macos_identity_fails() {
        let mut s = start_through_preflight();
        s.apply(MacosRecoveryEvent::EacsWipeFinished {
            ack_received: true,
            detail: "ack".into(),
        })
        .unwrap();
        s.apply(MacosRecoveryEvent::MdmReenrollFinished {
            success: true,
            profile_id: PROFILE.into(),
            detail: "ok".into(),
        })
        .unwrap();
        s.apply(MacosRecoveryEvent::HostIdentityRotated {
            new_host_identity: "MAC-HOST-SAME".into(),
            prior_host_identity: Some("MAC-HOST-SAME".into()),
        })
        .unwrap();
        assert!(s.phase.is_quarantined());
        let reason = s.quarantine_reason.as_deref().unwrap_or("");
        assert!(
            reason.contains("stale macos identity"),
            "reason={reason}"
        );
    }

    #[test]
    fn quarantine_persists_until_external_restore_or_dfu() {
        let mut s = start_through_preflight();
        s.apply(MacosRecoveryEvent::EacsWipeFinished {
            ack_received: false,
            detail: "timeout".into(),
        })
        .unwrap();
        assert!(s.phase.is_quarantined());
        s.apply(MacosRecoveryEvent::AttestationFinished(MacosAttestResult::ok()))
            .unwrap();
        assert!(s.phase.is_quarantined());
        assert!(!s.phase.allows_candidate_provision());
        s.apply(MacosRecoveryEvent::EgressCanaryFinished {
            denied: true,
            detail: "should-not-matter".into(),
        })
        .unwrap();
        assert!(s.phase.is_quarantined());
        assert!(
            s.trail
                .iter()
                .any(|t| t.contains("quarantine-persistence")),
            "trail={:?}",
            s.trail
        );

        s.apply(MacosRecoveryEvent::StartExternalRestore {
            external_controller: true,
        })
        .unwrap();
        assert!(matches!(
            s.phase,
            MacosRecoveryPhase::ExternalRecoveryControl
        ));
        assert!(s.quarantine_reason.is_none());
    }

    #[test]
    fn candidate_initiated_restore_rejected() {
        let mut s = MacosRecoveryState::new(PROFILE);
        s.apply(MacosRecoveryEvent::StartExternalRestore {
            external_controller: false,
        })
        .unwrap();
        assert!(s.phase.is_quarantined());
        assert!(!s.phase.allows_candidate_provision());
    }

    #[test]
    fn happy_path_eacs_ready_for_candidate() {
        let s = simulate_successful_macos_eacs_drill(
            PROFILE,
            "MAC-HOST-NEW",
            Some("MAC-HOST-OLD"),
        );
        assert!(s.phase.is_terminal_success(), "{:?}", s.phase);
        assert!(s.phase.allows_candidate_provision());
        assert_eq!(s.network, Some(MacosLabNetwork::Candidate));
        assert_eq!(s.reset_path.as_deref(), Some("eacs"));
        assert!(s.reset_completed);
        assert!(s.mdm_reenrolled);
        assert_eq!(s.host_identity.as_deref(), Some("MAC-HOST-NEW"));
        assert!(
            s.trail.iter().any(|t| t.contains("egress-canary-denied")),
            "trail={:?}",
            s.trail
        );
    }

    #[test]
    fn happy_path_dfu_fallback_after_missed_ack() {
        let s = simulate_successful_macos_dfu_drill(
            PROFILE,
            "MAC-HOST-DFU",
            Some("MAC-HOST-OLD"),
        );
        assert!(s.phase.is_terminal_success(), "{:?}", s.phase);
        assert!(s.phase.allows_candidate_provision());
        assert_eq!(s.reset_path.as_deref(), Some("dfu"));
        assert!(
            s.trail.iter().any(|t| t.contains("dfu-restore-ok")),
            "trail={:?}",
            s.trail
        );
        assert!(
            s.trail.iter().any(|t| t.contains("missed eacs ack")),
            "trail should show prior quarantine; trail={:?}",
            s.trail
        );
    }

    #[test]
    fn attestation_ssv_quarantine_blocks_provision() {
        let mut s = MacosRecoveryState::new(PROFILE);
        for ev in [
            MacosRecoveryEvent::StartExternalRestore {
                external_controller: true,
            },
            MacosRecoveryEvent::EacsPreflightFinished {
                ok: true,
                detail: "ok".into(),
            },
            MacosRecoveryEvent::EacsWipeFinished {
                ack_received: true,
                detail: "ack".into(),
            },
            MacosRecoveryEvent::MdmReenrollFinished {
                success: true,
                profile_id: PROFILE.into(),
                detail: "ok".into(),
            },
            MacosRecoveryEvent::HostIdentityRotated {
                new_host_identity: "id-new".into(),
                prior_host_identity: Some("id-old".into()),
            },
            MacosRecoveryEvent::NetworkMoved {
                network: MacosLabNetwork::Provisioning,
            },
            MacosRecoveryEvent::NetworkMoved {
                network: MacosLabNetwork::Candidate,
            },
        ] {
            s.apply(ev).unwrap();
        }
        s.apply(MacosRecoveryEvent::AttestationFinished(MacosAttestResult {
            verdict: MacosAttestVerdict::Quarantine,
            reasons: vec!["ssv invalid (seal failed)".into()],
        }))
        .unwrap();
        assert!(s.phase.is_quarantined());
        assert!(!s.phase.allows_candidate_provision());
    }
}
