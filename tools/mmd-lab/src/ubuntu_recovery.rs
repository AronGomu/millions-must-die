//! Ubuntu recovery protocol state machine (external controller only).
//!
//! Candidate OS never approves cleanup. Physical PXE/power/VLAN drill is
//! outside this module; unit tests exercise protocol transitions only.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ubuntu::{AttestResult, AttestVerdict};

/// Network segments the external controller moves the host through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LabNetwork {
    /// PXE / raw restore plane. No candidate code.
    Recovery,
    /// Post-restore provisioning (SSH key mint, package freeze). No internet.
    Provisioning,
    /// Candidate run plane. External controls must deny egress.
    Candidate,
}

impl fmt::Display for LabNetwork {
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
pub enum RecoveryPhase {
    /// No active restore. Awaiting external controller.
    Idle,
    /// Controller powered host into recovery path (PXE/iPXE).
    ExternalRecoveryBoot,
    /// Writing read-only raw image to disk.
    RestoringImage,
    /// Comparing controller readback digest to frozen image digest.
    VerifyingDigest,
    /// Minting fresh SSH host key + unprivileged user; old identity discarded.
    RotatingHostIdentity,
    /// Moving host across recovery → provisioning → candidate VLANs.
    NetworkTransition { from: LabNetwork, to: LabNetwork },
    /// Host attestation against frozen Ubuntu runner contract.
    Attesting,
    /// External egress canary from candidate VLAN must be denied.
    EgressCanary,
    /// Host ready for candidate archive dispatch (T17+).
    ReadyForCandidate,
    /// Drift / failed restore / stale identity / egress leak. No candidate.
    Quarantined { reason: String },
}

impl RecoveryPhase {
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
pub enum RecoveryEvent {
    /// Controller starts restore; candidate must not trigger this.
    StartExternalRestore {
        /// True when start originates from external controller (required).
        external_controller: bool,
    },
    /// PXE/recovery environment reached.
    RecoveryBootConfirmed,
    /// Raw image write finished; controller supplies readback digest.
    ImageWriteFinished { readback_digest_sha256: String },
    /// Fresh SSH host identity observed after rotate.
    HostIdentityRotated {
        new_host_key_fp: String,
        /// Fingerprint seen before rotate (must not equal new).
        prior_host_key_fp: Option<String>,
    },
    /// Controller completed a network move.
    NetworkMoved { network: LabNetwork },
    /// Result of host attestation (coordinator-side, not candidate).
    AttestationFinished(AttestResult),
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
pub struct RecoveryState {
    pub phase: RecoveryPhase,
    /// Frozen expected whole-disk image digest (from image-manifest / runner manifest).
    pub expected_image_digest_sha256: String,
    /// Last verified readback digest (Some only after successful verify).
    pub verified_readback_digest: Option<String>,
    /// Current host key fingerprint after rotate.
    pub host_key_fp: Option<String>,
    /// Fingerprint that must not reappear after rotate.
    pub retired_host_key_fp: Option<String>,
    /// Network the controller last confirmed.
    pub network: Option<LabNetwork>,
    /// Ordered log of phase names for drill evidence.
    pub trail: Vec<String>,
    /// Quarantine sticky until external restore clears it.
    pub quarantine_reason: Option<String>,
}

impl RecoveryState {
    pub fn new(expected_image_digest_sha256: impl Into<String>) -> Self {
        Self {
            phase: RecoveryPhase::Idle,
            expected_image_digest_sha256: expected_image_digest_sha256.into().to_ascii_lowercase(),
            verified_readback_digest: None,
            host_key_fp: None,
            retired_host_key_fp: None,
            network: None,
            trail: vec!["idle".into()],
            quarantine_reason: None,
        }
    }

    fn quarantine(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        self.quarantine_reason = Some(reason.clone());
        self.phase = RecoveryPhase::Quarantined {
            reason: reason.clone(),
        };
        self.trail.push(format!("quarantined:{reason}"));
        // Clear ready markers so provision cannot proceed on sticky quarantine.
        self.verified_readback_digest = None;
    }

    fn enter(&mut self, phase: RecoveryPhase) {
        let label = match &phase {
            RecoveryPhase::Idle => "idle".into(),
            RecoveryPhase::ExternalRecoveryBoot => "external-recovery-boot".into(),
            RecoveryPhase::RestoringImage => "restoring-image".into(),
            RecoveryPhase::VerifyingDigest => "verifying-digest".into(),
            RecoveryPhase::RotatingHostIdentity => "rotating-host-identity".into(),
            RecoveryPhase::NetworkTransition { from, to } => {
                format!("network-transition:{from}->{to}")
            }
            RecoveryPhase::Attesting => "attesting".into(),
            RecoveryPhase::EgressCanary => "egress-canary".into(),
            RecoveryPhase::ReadyForCandidate => "ready-for-candidate".into(),
            RecoveryPhase::Quarantined { reason } => format!("quarantined:{reason}"),
        };
        self.trail.push(label);
        self.phase = phase;
    }

    /// Apply one external-controller event. Returns error string only for
    /// programmer misuse (event in wrong phase); policy failures quarantine.
    pub fn apply(&mut self, event: RecoveryEvent) -> Result<(), String> {
        // Sticky quarantine: only a fresh external restore may leave it.
        if let RecoveryPhase::Quarantined { .. } = &self.phase {
            match &event {
                RecoveryEvent::StartExternalRestore {
                    external_controller: true,
                } => {
                    // Clear sticky quarantine and restart protocol.
                    self.quarantine_reason = None;
                    self.host_key_fp = None;
                    self.retired_host_key_fp = None;
                    self.verified_readback_digest = None;
                    self.network = None;
                    self.enter(RecoveryPhase::ExternalRecoveryBoot);
                    return Ok(());
                }
                RecoveryEvent::ForceQuarantine { reason } => {
                    self.quarantine(reason.clone());
                    return Ok(());
                }
                _ => {
                    // Persist quarantine; ignore other events (no provision).
                    self.trail
                        .push("quarantine-persistence:ignored-non-restore-event".into());
                    return Ok(());
                }
            }
        }

        match event {
            RecoveryEvent::StartExternalRestore {
                external_controller,
            } => {
                if !external_controller {
                    self.quarantine("candidate-initiated restore rejected");
                    return Ok(());
                }
                match self.phase {
                    RecoveryPhase::Idle
                    | RecoveryPhase::ReadyForCandidate
                    | RecoveryPhase::ExternalRecoveryBoot => {
                        self.verified_readback_digest = None;
                        self.host_key_fp = None;
                        self.retired_host_key_fp = None;
                        self.network = None;
                        self.enter(RecoveryPhase::ExternalRecoveryBoot);
                        Ok(())
                    }
                    ref other => Err(format!("StartExternalRestore invalid in phase {other:?}")),
                }
            }

            RecoveryEvent::RecoveryBootConfirmed => {
                if self.phase != RecoveryPhase::ExternalRecoveryBoot {
                    return Err(format!(
                        "RecoveryBootConfirmed invalid in phase {:?}",
                        self.phase
                    ));
                }
                self.network = Some(LabNetwork::Recovery);
                self.enter(RecoveryPhase::RestoringImage);
                Ok(())
            }

            RecoveryEvent::ImageWriteFinished {
                readback_digest_sha256,
            } => {
                if self.phase != RecoveryPhase::RestoringImage {
                    return Err(format!(
                        "ImageWriteFinished invalid in phase {:?}",
                        self.phase
                    ));
                }
                self.enter(RecoveryPhase::VerifyingDigest);
                let got = readback_digest_sha256.to_ascii_lowercase();
                if !is_sha256_hex(&got) {
                    self.quarantine(format!("readback digest not sha256 hex: {got}"));
                    return Ok(());
                }
                if got != self.expected_image_digest_sha256 {
                    self.quarantine(format!(
                        "restore digest mismatch: readback {got} != expected {}",
                        self.expected_image_digest_sha256
                    ));
                    return Ok(());
                }
                self.verified_readback_digest = Some(got);
                self.enter(RecoveryPhase::RotatingHostIdentity);
                Ok(())
            }

            RecoveryEvent::HostIdentityRotated {
                new_host_key_fp,
                prior_host_key_fp,
            } => {
                if self.phase != RecoveryPhase::RotatingHostIdentity {
                    return Err(format!(
                        "HostIdentityRotated invalid in phase {:?}",
                        self.phase
                    ));
                }
                if new_host_key_fp.trim().is_empty() {
                    self.quarantine("empty host key fingerprint after rotate");
                    return Ok(());
                }
                if let Some(prior) = prior_host_key_fp.as_ref() {
                    if prior == &new_host_key_fp {
                        self.quarantine("stale host key: fingerprint unchanged after rotate");
                        return Ok(());
                    }
                    self.retired_host_key_fp = Some(prior.clone());
                }
                // Reject reuse of any known retired key.
                if self.retired_host_key_fp.as_ref() == Some(&new_host_key_fp) {
                    self.quarantine("stale host key: retired fingerprint reused");
                    return Ok(());
                }
                self.host_key_fp = Some(new_host_key_fp);
                // Next: recovery → provisioning.
                self.enter(RecoveryPhase::NetworkTransition {
                    from: LabNetwork::Recovery,
                    to: LabNetwork::Provisioning,
                });
                Ok(())
            }

            RecoveryEvent::NetworkMoved { network } => match &self.phase {
                RecoveryPhase::NetworkTransition { from, to } => {
                    if network != *to {
                        self.quarantine(format!(
                            "network move to {network} != expected {to} (from {from})"
                        ));
                        return Ok(());
                    }
                    self.network = Some(network);
                    match network {
                        LabNetwork::Provisioning => {
                            // provisioning → candidate next
                            self.enter(RecoveryPhase::NetworkTransition {
                                from: LabNetwork::Provisioning,
                                to: LabNetwork::Candidate,
                            });
                        }
                        LabNetwork::Candidate => {
                            self.enter(RecoveryPhase::Attesting);
                        }
                        LabNetwork::Recovery => {
                            self.quarantine("unexpected return to recovery mid-transition");
                        }
                    }
                    Ok(())
                }
                other => Err(format!("NetworkMoved invalid in phase {other:?}")),
            },

            RecoveryEvent::AttestationFinished(result) => {
                if self.phase != RecoveryPhase::Attesting {
                    return Err(format!(
                        "AttestationFinished invalid in phase {:?}",
                        self.phase
                    ));
                }
                match result.verdict {
                    AttestVerdict::ReadyForRecovery => {
                        self.enter(RecoveryPhase::EgressCanary);
                        Ok(())
                    }
                    AttestVerdict::Quarantine | AttestVerdict::Reject => {
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

            RecoveryEvent::EgressCanaryFinished { denied, detail } => {
                if self.phase != RecoveryPhase::EgressCanary {
                    return Err(format!(
                        "EgressCanaryFinished invalid in phase {:?}",
                        self.phase
                    ));
                }
                if !denied {
                    self.quarantine(format!("egress canary not denied: {detail}"));
                    return Ok(());
                }
                // Require candidate network confirmed before ready.
                if self.network != Some(LabNetwork::Candidate) {
                    self.quarantine("egress canary without candidate network");
                    return Ok(());
                }
                if self.verified_readback_digest.is_none() || self.host_key_fp.is_none() {
                    self.quarantine("egress canary missing digest or host identity");
                    return Ok(());
                }
                self.trail.push(format!("egress-canary-denied:{detail}"));
                self.enter(RecoveryPhase::ReadyForCandidate);
                Ok(())
            }

            RecoveryEvent::ForceQuarantine { reason } => {
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
/// `prior_host_key_fp` must differ from `new_host_key_fp` when provided.
pub fn simulate_successful_drill(
    expected_digest: &str,
    new_host_key_fp: &str,
    prior_host_key_fp: Option<&str>,
) -> RecoveryState {
    let mut s = RecoveryState::new(expected_digest);
    let steps = [
        RecoveryEvent::StartExternalRestore {
            external_controller: true,
        },
        RecoveryEvent::RecoveryBootConfirmed,
        RecoveryEvent::ImageWriteFinished {
            readback_digest_sha256: expected_digest.to_string(),
        },
        RecoveryEvent::HostIdentityRotated {
            new_host_key_fp: new_host_key_fp.to_string(),
            prior_host_key_fp: prior_host_key_fp.map(str::to_string),
        },
        RecoveryEvent::NetworkMoved {
            network: LabNetwork::Provisioning,
        },
        RecoveryEvent::NetworkMoved {
            network: LabNetwork::Candidate,
        },
        RecoveryEvent::AttestationFinished(AttestResult::ok()),
        RecoveryEvent::EgressCanaryFinished {
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
    use crate::ubuntu::AttestResult;

    const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn start_through_restore(digest: &str) -> RecoveryState {
        let mut s = RecoveryState::new(DIGEST_A);
        s.apply(RecoveryEvent::StartExternalRestore {
            external_controller: true,
        })
        .unwrap();
        s.apply(RecoveryEvent::RecoveryBootConfirmed).unwrap();
        s.apply(RecoveryEvent::ImageWriteFinished {
            readback_digest_sha256: digest.into(),
        })
        .unwrap();
        s
    }

    #[test]
    fn restore_digest_mismatch_quarantines() {
        let s = start_through_restore(DIGEST_B);
        assert!(s.phase.is_quarantined(), "{:?}", s.phase);
        assert!(!s.phase.allows_candidate_provision());
        assert!(s.verified_readback_digest.is_none());
        let reason = s.quarantine_reason.as_deref().unwrap_or("");
        assert!(reason.contains("digest mismatch"), "reason={reason}");
    }

    #[test]
    fn stale_host_key_fails() {
        let mut s = start_through_restore(DIGEST_A);
        assert!(matches!(s.phase, RecoveryPhase::RotatingHostIdentity));
        s.apply(RecoveryEvent::HostIdentityRotated {
            new_host_key_fp: "ssh-ed25519 AAAAold".into(),
            prior_host_key_fp: Some("ssh-ed25519 AAAAold".into()),
        })
        .unwrap();
        assert!(s.phase.is_quarantined(), "{:?}", s.phase);
        assert!(!s.phase.allows_candidate_provision());
        let reason = s.quarantine_reason.as_deref().unwrap_or("");
        assert!(reason.contains("stale host key"), "reason={reason}");
    }

    #[test]
    fn egress_canary_blocks() {
        let mut s = RecoveryState::new(DIGEST_A);
        for ev in [
            RecoveryEvent::StartExternalRestore {
                external_controller: true,
            },
            RecoveryEvent::RecoveryBootConfirmed,
            RecoveryEvent::ImageWriteFinished {
                readback_digest_sha256: DIGEST_A.into(),
            },
            RecoveryEvent::HostIdentityRotated {
                new_host_key_fp: "ssh-ed25519 AAAAnew".into(),
                prior_host_key_fp: Some("ssh-ed25519 AAAAold".into()),
            },
            RecoveryEvent::NetworkMoved {
                network: LabNetwork::Provisioning,
            },
            RecoveryEvent::NetworkMoved {
                network: LabNetwork::Candidate,
            },
            RecoveryEvent::AttestationFinished(AttestResult::ok()),
        ] {
            s.apply(ev).unwrap();
        }
        assert!(matches!(s.phase, RecoveryPhase::EgressCanary));
        s.apply(RecoveryEvent::EgressCanaryFinished {
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
        let mut s = start_through_restore(DIGEST_B);
        assert!(s.phase.is_quarantined());
        // Attestation / network / egress must not clear quarantine or allow provision.
        s.apply(RecoveryEvent::AttestationFinished(AttestResult::ok()))
            .unwrap();
        assert!(s.phase.is_quarantined());
        assert!(!s.phase.allows_candidate_provision());
        s.apply(RecoveryEvent::NetworkMoved {
            network: LabNetwork::Candidate,
        })
        .unwrap();
        assert!(s.phase.is_quarantined());
        s.apply(RecoveryEvent::EgressCanaryFinished {
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

        // Only external restore restarts.
        s.apply(RecoveryEvent::StartExternalRestore {
            external_controller: true,
        })
        .unwrap();
        assert!(matches!(s.phase, RecoveryPhase::ExternalRecoveryBoot));
        assert!(s.quarantine_reason.is_none());
    }

    #[test]
    fn candidate_initiated_restore_rejected() {
        let mut s = RecoveryState::new(DIGEST_A);
        s.apply(RecoveryEvent::StartExternalRestore {
            external_controller: false,
        })
        .unwrap();
        assert!(s.phase.is_quarantined());
        assert!(!s.phase.allows_candidate_provision());
    }

    #[test]
    fn happy_path_ready_for_candidate() {
        let s =
            simulate_successful_drill(DIGEST_A, "ssh-ed25519 AAAAnew", Some("ssh-ed25519 AAAAold"));
        assert!(s.phase.is_terminal_success(), "{:?}", s.phase);
        assert!(s.phase.allows_candidate_provision());
        assert_eq!(s.network, Some(LabNetwork::Candidate));
        assert_eq!(s.verified_readback_digest.as_deref(), Some(DIGEST_A));
        assert_eq!(s.host_key_fp.as_deref(), Some("ssh-ed25519 AAAAnew"));
        assert!(
            s.trail.iter().any(|t| t.contains("egress-canary-denied")),
            "trail={:?}",
            s.trail
        );
    }

    #[test]
    fn force_quarantine_sticky() {
        let mut s = RecoveryState::new(DIGEST_A);
        s.apply(RecoveryEvent::ForceQuarantine {
            reason: "manual drift".into(),
        })
        .unwrap();
        assert!(s.phase.is_quarantined());
        s.apply(RecoveryEvent::EgressCanaryFinished {
            denied: true,
            detail: "n/a".into(),
        })
        .unwrap();
        assert!(s.phase.is_quarantined());
    }

    #[test]
    fn attestation_quarantine_blocks_provision() {
        let mut s = RecoveryState::new(DIGEST_A);
        for ev in [
            RecoveryEvent::StartExternalRestore {
                external_controller: true,
            },
            RecoveryEvent::RecoveryBootConfirmed,
            RecoveryEvent::ImageWriteFinished {
                readback_digest_sha256: DIGEST_A.into(),
            },
            RecoveryEvent::HostIdentityRotated {
                new_host_key_fp: "k-new".into(),
                prior_host_key_fp: Some("k-old".into()),
            },
            RecoveryEvent::NetworkMoved {
                network: LabNetwork::Provisioning,
            },
            RecoveryEvent::NetworkMoved {
                network: LabNetwork::Candidate,
            },
        ] {
            s.apply(ev).unwrap();
        }
        s.apply(RecoveryEvent::AttestationFinished(AttestResult {
            verdict: AttestVerdict::Quarantine,
            reasons: vec!["vbios drift".into()],
        }))
        .unwrap();
        assert!(s.phase.is_quarantined());
        assert!(!s.phase.allows_candidate_provision());
    }
}
