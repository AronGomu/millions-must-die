//! Windows recovery protocol state machine (external controller only).
//!
//! Candidate OS never approves cleanup. Physical WinPE/FFU/power/VLAN drill is
//! outside this module; unit tests exercise protocol transitions only.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::windows::{WindowsAttestResult, WindowsAttestVerdict};

/// Network segments the external controller moves the host through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowsLabNetwork {
    /// WinPE / FFU restore plane. No candidate code.
    Recovery,
    /// Post-restore provisioning (fresh identity, power plan freeze). No internet.
    Provisioning,
    /// Candidate run plane. External controls must deny egress.
    Candidate,
}

impl fmt::Display for WindowsLabNetwork {
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
pub enum WindowsRecoveryPhase {
    /// No active restore. Awaiting external controller.
    Idle,
    /// Controller powered host into WinPE recovery path.
    ExternalWinPeBoot,
    /// DISM `/Apply-FFU` full-drive write in progress.
    ApplyingFfu,
    /// Comparing controller readback digest to frozen FFU digest.
    VerifyingDigest,
    /// Minting fresh host identity/user; old identity discarded.
    RotatingHostIdentity,
    /// Moving host across recovery → provisioning → candidate VLANs.
    NetworkTransition {
        from: WindowsLabNetwork,
        to: WindowsLabNetwork,
    },
    /// Host attestation against frozen Windows runner contract.
    Attesting,
    /// External egress canary from candidate VLAN must be denied.
    EgressCanary,
    /// Host ready for candidate archive dispatch (T20+).
    ReadyForCandidate,
    /// Drift / failed FFU / stale identity / egress leak. No candidate.
    Quarantined { reason: String },
}

impl WindowsRecoveryPhase {
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

/// External controller observations. Never supplied by candidate OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsRecoveryEvent {
    /// Controller starts restore; candidate must not trigger this.
    StartExternalRestore {
        /// True when start originates from external controller (required).
        external_controller: bool,
    },
    /// WinPE recovery environment reached.
    WinPeBootConfirmed,
    /// DISM FFU apply finished; controller supplies success + readback digest.
    FfuApplyFinished {
        /// False when DISM fails → quarantine (no provision).
        success: bool,
        /// Whole-disk readback SHA-256 after apply (ignored when `success` is false).
        readback_digest_sha256: String,
        detail: String,
    },
    /// Fresh Windows host identity observed after rotate.
    HostIdentityRotated {
        new_host_identity: String,
        /// Identity seen before rotate (must not equal new).
        prior_host_identity: Option<String>,
    },
    /// Controller completed a network move.
    NetworkMoved { network: WindowsLabNetwork },
    /// Result of host attestation (coordinator-side, not candidate).
    AttestationFinished(WindowsAttestResult),
    /// External egress canary result on candidate VLAN.
    EgressCanaryFinished {
        /// True when probe was denied by external controls (required pass).
        denied: bool,
        detail: String,
    },
    /// Explicit re-quarantine (manual or automated drift watch).
    #[allow(dead_code)] // controller / ops path; exercised in unit tests below
    ForceQuarantine { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsRecoveryState {
    pub phase: WindowsRecoveryPhase,
    /// Frozen expected whole-disk FFU digest (from image-manifest / runner manifest).
    pub expected_ffu_digest_sha256: String,
    /// Last verified readback digest (Some only after successful verify).
    pub verified_readback_digest: Option<String>,
    /// Current host identity after rotate.
    pub host_identity: Option<String>,
    /// Identity that must not reappear after rotate.
    pub retired_host_identity: Option<String>,
    /// Network the controller last confirmed.
    pub network: Option<WindowsLabNetwork>,
    /// Ordered log of phase names for drill evidence.
    pub trail: Vec<String>,
    /// Quarantine sticky until external restore clears it.
    pub quarantine_reason: Option<String>,
}

impl WindowsRecoveryState {
    pub fn new(expected_ffu_digest_sha256: impl Into<String>) -> Self {
        Self {
            phase: WindowsRecoveryPhase::Idle,
            expected_ffu_digest_sha256: expected_ffu_digest_sha256.into().to_ascii_lowercase(),
            verified_readback_digest: None,
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
        self.phase = WindowsRecoveryPhase::Quarantined {
            reason: reason.clone(),
        };
        self.trail.push(format!("quarantined:{reason}"));
        // Clear ready markers so provision cannot proceed on sticky quarantine.
        self.verified_readback_digest = None;
    }

    fn enter(&mut self, phase: WindowsRecoveryPhase) {
        let label = match &phase {
            WindowsRecoveryPhase::Idle => "idle".into(),
            WindowsRecoveryPhase::ExternalWinPeBoot => "external-winpe-boot".into(),
            WindowsRecoveryPhase::ApplyingFfu => "applying-ffu".into(),
            WindowsRecoveryPhase::VerifyingDigest => "verifying-digest".into(),
            WindowsRecoveryPhase::RotatingHostIdentity => "rotating-host-identity".into(),
            WindowsRecoveryPhase::NetworkTransition { from, to } => {
                format!("network-transition:{from}->{to}")
            }
            WindowsRecoveryPhase::Attesting => "attesting".into(),
            WindowsRecoveryPhase::EgressCanary => "egress-canary".into(),
            WindowsRecoveryPhase::ReadyForCandidate => "ready-for-candidate".into(),
            WindowsRecoveryPhase::Quarantined { reason } => format!("quarantined:{reason}"),
        };
        self.trail.push(label);
        self.phase = phase;
    }

    /// Apply one external-controller event. Returns error string only for
    /// programmer misuse (event in wrong phase); policy failures quarantine.
    pub fn apply(&mut self, event: WindowsRecoveryEvent) -> Result<(), String> {
        // Sticky quarantine: only a fresh external restore may leave it.
        if let WindowsRecoveryPhase::Quarantined { .. } = &self.phase {
            match &event {
                WindowsRecoveryEvent::StartExternalRestore {
                    external_controller: true,
                } => {
                    self.quarantine_reason = None;
                    self.host_identity = None;
                    self.retired_host_identity = None;
                    self.verified_readback_digest = None;
                    self.network = None;
                    self.enter(WindowsRecoveryPhase::ExternalWinPeBoot);
                    return Ok(());
                }
                WindowsRecoveryEvent::ForceQuarantine { reason } => {
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
            WindowsRecoveryEvent::StartExternalRestore {
                external_controller,
            } => {
                if !external_controller {
                    self.quarantine("candidate-initiated restore rejected");
                    return Ok(());
                }
                match self.phase {
                    WindowsRecoveryPhase::Idle
                    | WindowsRecoveryPhase::ReadyForCandidate
                    | WindowsRecoveryPhase::ExternalWinPeBoot => {
                        self.verified_readback_digest = None;
                        self.host_identity = None;
                        self.retired_host_identity = None;
                        self.network = None;
                        self.enter(WindowsRecoveryPhase::ExternalWinPeBoot);
                        Ok(())
                    }
                    ref other => Err(format!("StartExternalRestore invalid in phase {other:?}")),
                }
            }

            WindowsRecoveryEvent::WinPeBootConfirmed => {
                if self.phase != WindowsRecoveryPhase::ExternalWinPeBoot {
                    return Err(format!(
                        "WinPeBootConfirmed invalid in phase {:?}",
                        self.phase
                    ));
                }
                self.network = Some(WindowsLabNetwork::Recovery);
                self.enter(WindowsRecoveryPhase::ApplyingFfu);
                Ok(())
            }

            WindowsRecoveryEvent::FfuApplyFinished {
                success,
                readback_digest_sha256,
                detail,
            } => {
                if self.phase != WindowsRecoveryPhase::ApplyingFfu {
                    return Err(format!(
                        "FfuApplyFinished invalid in phase {:?}",
                        self.phase
                    ));
                }
                if !success {
                    self.quarantine(format!("ffu apply failure: {detail}"));
                    return Ok(());
                }
                self.enter(WindowsRecoveryPhase::VerifyingDigest);
                let got = readback_digest_sha256.to_ascii_lowercase();
                if !is_sha256_hex(&got) {
                    self.quarantine(format!("readback digest not sha256 hex: {got}"));
                    return Ok(());
                }
                if got != self.expected_ffu_digest_sha256 {
                    self.quarantine(format!(
                        "ffu digest mismatch: readback {got} != expected {}",
                        self.expected_ffu_digest_sha256
                    ));
                    return Ok(());
                }
                self.verified_readback_digest.replace(got);
                self.enter(WindowsRecoveryPhase::RotatingHostIdentity);
                Ok(())
            }

            WindowsRecoveryEvent::HostIdentityRotated {
                new_host_identity,
                prior_host_identity,
            } => {
                if self.phase != WindowsRecoveryPhase::RotatingHostIdentity {
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
                            "stale windows identity: fingerprint unchanged after rotate",
                        );
                        return Ok(());
                    }
                    self.retired_host_identity = Some(prior.clone());
                }
                if self.retired_host_identity.as_ref() == Some(&new_host_identity) {
                    self.quarantine("stale windows identity: retired fingerprint reused");
                    return Ok(());
                }
                self.host_identity = Some(new_host_identity);
                self.enter(WindowsRecoveryPhase::NetworkTransition {
                    from: WindowsLabNetwork::Recovery,
                    to: WindowsLabNetwork::Provisioning,
                });
                Ok(())
            }

            WindowsRecoveryEvent::NetworkMoved { network } => match &self.phase {
                WindowsRecoveryPhase::NetworkTransition { from, to } => {
                    if network != *to {
                        self.quarantine(format!(
                            "network move to {network} != expected {to} (from {from})"
                        ));
                        return Ok(());
                    }
                    self.network = Some(network);
                    match network {
                        WindowsLabNetwork::Provisioning => {
                            self.enter(WindowsRecoveryPhase::NetworkTransition {
                                from: WindowsLabNetwork::Provisioning,
                                to: WindowsLabNetwork::Candidate,
                            });
                        }
                        WindowsLabNetwork::Candidate => {
                            self.enter(WindowsRecoveryPhase::Attesting);
                        }
                        WindowsLabNetwork::Recovery => {
                            self.quarantine("unexpected return to recovery mid-transition");
                        }
                    }
                    Ok(())
                }
                other => Err(format!("NetworkMoved invalid in phase {other:?}")),
            },

            WindowsRecoveryEvent::AttestationFinished(result) => {
                if self.phase != WindowsRecoveryPhase::Attesting {
                    return Err(format!(
                        "AttestationFinished invalid in phase {:?}",
                        self.phase
                    ));
                }
                match result.verdict {
                    WindowsAttestVerdict::ReadyForRecovery => {
                        self.enter(WindowsRecoveryPhase::EgressCanary);
                        Ok(())
                    }
                    WindowsAttestVerdict::Quarantine
                    | WindowsAttestVerdict::Reject
                    | WindowsAttestVerdict::MaintenanceBlock => {
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

            WindowsRecoveryEvent::EgressCanaryFinished { denied, detail } => {
                if self.phase != WindowsRecoveryPhase::EgressCanary {
                    return Err(format!(
                        "EgressCanaryFinished invalid in phase {:?}",
                        self.phase
                    ));
                }
                if !denied {
                    self.quarantine(format!("egress canary not denied: {detail}"));
                    return Ok(());
                }
                if self.network != Some(WindowsLabNetwork::Candidate) {
                    self.quarantine("egress canary without candidate network");
                    return Ok(());
                }
                if self.verified_readback_digest.is_none() || self.host_identity.is_none() {
                    self.quarantine("egress canary missing digest or host identity");
                    return Ok(());
                }
                self.trail.push(format!("egress-canary-denied:{detail}"));
                self.enter(WindowsRecoveryPhase::ReadyForCandidate);
                Ok(())
            }

            WindowsRecoveryEvent::ForceQuarantine { reason } => {
                self.quarantine(reason);
                Ok(())
            }
        }
    }
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Canonical happy-path event sequence for unit/simulation drills.
#[cfg_attr(not(test), allow(dead_code))]
pub fn simulate_successful_windows_drill(
    expected_digest: &str,
    new_host_identity: &str,
    prior_host_identity: Option<&str>,
) -> WindowsRecoveryState {
    let mut s = WindowsRecoveryState::new(expected_digest);
    let steps = [
        WindowsRecoveryEvent::StartExternalRestore {
            external_controller: true,
        },
        WindowsRecoveryEvent::WinPeBootConfirmed,
        WindowsRecoveryEvent::FfuApplyFinished {
            success: true,
            readback_digest_sha256: expected_digest.to_string(),
            detail: "dism-apply-ok".into(),
        },
        WindowsRecoveryEvent::HostIdentityRotated {
            new_host_identity: new_host_identity.to_string(),
            prior_host_identity: prior_host_identity.map(str::to_string),
        },
        WindowsRecoveryEvent::NetworkMoved {
            network: WindowsLabNetwork::Provisioning,
        },
        WindowsRecoveryEvent::NetworkMoved {
            network: WindowsLabNetwork::Candidate,
        },
        WindowsRecoveryEvent::AttestationFinished(WindowsAttestResult::ok()),
        WindowsRecoveryEvent::EgressCanaryFinished {
            denied: true,
            detail: "external-flow-log:tcp/443+dns blocked".into(),
        },
    ];
    for ev in steps {
        s.apply(ev).expect("happy path event");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::WindowsAttestResult;

    const DIGEST_A: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const DIGEST_B: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

    fn start_through_ffu(digest: &str, success: bool) -> WindowsRecoveryState {
        let mut s = WindowsRecoveryState::new(DIGEST_A);
        s.apply(WindowsRecoveryEvent::StartExternalRestore {
            external_controller: true,
        })
        .unwrap();
        s.apply(WindowsRecoveryEvent::WinPeBootConfirmed).unwrap();
        s.apply(WindowsRecoveryEvent::FfuApplyFinished {
            success,
            readback_digest_sha256: digest.into(),
            detail: if success {
                "dism-ok".into()
            } else {
                "DISM /Apply-FFU exit 87".into()
            },
        })
        .unwrap();
        s
    }

    #[test]
    fn ffu_apply_failure_quarantines() {
        let s = start_through_ffu(DIGEST_A, false);
        assert!(s.phase.is_quarantined(), "{:?}", s.phase);
        assert!(!s.phase.allows_candidate_provision());
        assert!(s.verified_readback_digest.is_none());
        let reason = s.quarantine_reason.as_deref().unwrap_or("");
        assert!(reason.contains("ffu apply failure"), "reason={reason}");
    }

    #[test]
    fn ffu_digest_mismatch_quarantines() {
        let s = start_through_ffu(DIGEST_B, true);
        assert!(s.phase.is_quarantined(), "{:?}", s.phase);
        assert!(!s.phase.allows_candidate_provision());
        let reason = s.quarantine_reason.as_deref().unwrap_or("");
        assert!(reason.contains("digest mismatch"), "reason={reason}");
    }

    #[test]
    fn stale_windows_identity_fails() {
        let mut s = start_through_ffu(DIGEST_A, true);
        assert!(matches!(
            s.phase,
            WindowsRecoveryPhase::RotatingHostIdentity
        ));
        s.apply(WindowsRecoveryEvent::HostIdentityRotated {
            new_host_identity: "WIN-MACHINE-OLD".into(),
            prior_host_identity: Some("WIN-MACHINE-OLD".into()),
        })
        .unwrap();
        assert!(s.phase.is_quarantined(), "{:?}", s.phase);
        assert!(!s.phase.allows_candidate_provision());
        let reason = s.quarantine_reason.as_deref().unwrap_or("");
        assert!(reason.contains("stale windows identity"), "reason={reason}");
    }

    #[test]
    fn windows_egress_canary_blocks() {
        let mut s = WindowsRecoveryState::new(DIGEST_A);
        for ev in [
            WindowsRecoveryEvent::StartExternalRestore {
                external_controller: true,
            },
            WindowsRecoveryEvent::WinPeBootConfirmed,
            WindowsRecoveryEvent::FfuApplyFinished {
                success: true,
                readback_digest_sha256: DIGEST_A.into(),
                detail: "ok".into(),
            },
            WindowsRecoveryEvent::HostIdentityRotated {
                new_host_identity: "WIN-MACHINE-NEW".into(),
                prior_host_identity: Some("WIN-MACHINE-OLD".into()),
            },
            WindowsRecoveryEvent::NetworkMoved {
                network: WindowsLabNetwork::Provisioning,
            },
            WindowsRecoveryEvent::NetworkMoved {
                network: WindowsLabNetwork::Candidate,
            },
            WindowsRecoveryEvent::AttestationFinished(WindowsAttestResult::ok()),
        ] {
            s.apply(ev).unwrap();
        }
        assert!(matches!(s.phase, WindowsRecoveryPhase::EgressCanary));
        s.apply(WindowsRecoveryEvent::EgressCanaryFinished {
            denied: false,
            detail: "canary reached 1.1.1.1:443".into(),
        })
        .unwrap();
        assert!(s.phase.is_quarantined(), "{:?}", s.phase);
        assert!(!s.phase.allows_candidate_provision());
        let reason = s.quarantine_reason.as_deref().unwrap_or("");
        assert!(reason.contains("egress canary"), "reason={reason}");
    }

    #[test]
    fn quarantine_persists_until_external_restore() {
        let mut s = start_through_ffu(DIGEST_A, false);
        assert!(s.phase.is_quarantined());
        s.apply(WindowsRecoveryEvent::AttestationFinished(
            WindowsAttestResult::ok(),
        ))
        .unwrap();
        assert!(s.phase.is_quarantined());
        assert!(!s.phase.allows_candidate_provision());
        s.apply(WindowsRecoveryEvent::NetworkMoved {
            network: WindowsLabNetwork::Candidate,
        })
        .unwrap();
        assert!(s.phase.is_quarantined());
        s.apply(WindowsRecoveryEvent::EgressCanaryFinished {
            denied: true,
            detail: "should-not-matter".into(),
        })
        .unwrap();
        assert!(s.phase.is_quarantined());
        assert!(
            s.trail.iter().any(|t| t.contains("quarantine-persistence")),
            "trail={:?}",
            s.trail
        );

        s.apply(WindowsRecoveryEvent::StartExternalRestore {
            external_controller: true,
        })
        .unwrap();
        assert!(matches!(s.phase, WindowsRecoveryPhase::ExternalWinPeBoot));
        assert!(s.quarantine_reason.is_none());
    }

    #[test]
    fn candidate_initiated_restore_rejected() {
        let mut s = WindowsRecoveryState::new(DIGEST_A);
        s.apply(WindowsRecoveryEvent::StartExternalRestore {
            external_controller: false,
        })
        .unwrap();
        assert!(s.phase.is_quarantined());
        assert!(!s.phase.allows_candidate_provision());
    }

    #[test]
    fn happy_path_ready_for_candidate() {
        let s =
            simulate_successful_windows_drill(DIGEST_A, "WIN-MACHINE-NEW", Some("WIN-MACHINE-OLD"));
        assert!(s.phase.is_terminal_success(), "{:?}", s.phase);
        assert!(s.phase.allows_candidate_provision());
        assert_eq!(s.network, Some(WindowsLabNetwork::Candidate));
        assert_eq!(s.verified_readback_digest.as_deref(), Some(DIGEST_A));
        assert_eq!(s.host_identity.as_deref(), Some("WIN-MACHINE-NEW"));
        assert!(
            s.trail.iter().any(|t| t.contains("egress-canary-denied")),
            "trail={:?}",
            s.trail
        );
    }

    #[test]
    fn attestation_reject_blocks_provision() {
        let mut s = WindowsRecoveryState::new(DIGEST_A);
        for ev in [
            WindowsRecoveryEvent::StartExternalRestore {
                external_controller: true,
            },
            WindowsRecoveryEvent::WinPeBootConfirmed,
            WindowsRecoveryEvent::FfuApplyFinished {
                success: true,
                readback_digest_sha256: DIGEST_A.into(),
                detail: "ok".into(),
            },
            WindowsRecoveryEvent::HostIdentityRotated {
                new_host_identity: "id-new".into(),
                prior_host_identity: Some("id-old".into()),
            },
            WindowsRecoveryEvent::NetworkMoved {
                network: WindowsLabNetwork::Provisioning,
            },
            WindowsRecoveryEvent::NetworkMoved {
                network: WindowsLabNetwork::Candidate,
            },
        ] {
            s.apply(ev).unwrap();
        }
        s.apply(WindowsRecoveryEvent::AttestationFinished(
            WindowsAttestResult {
                verdict: WindowsAttestVerdict::Reject,
                reasons: vec!["basic render driver".into()],
            },
        ))
        .unwrap();
        assert!(s.phase.is_quarantined());
        assert!(!s.phase.allows_candidate_provision());
    }
}
