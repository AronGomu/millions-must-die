//! Agent transport trait: fake fixtures + real SSH (stub for later tickets).

use std::fs;
use std::path::Path;

use thiserror::Error;

use crate::archive::{verify_bytes_hash, ArchiveBlob, ArchiveError};
use crate::report::HostEvidence;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("archive: {0}")]
    Archive(#[from] ArchiveError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("transport: {0}")]
    Msg(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Deliver content-addressed archive; collect host evidence.
#[allow(dead_code)] // trait surface for fake + future SSH
pub trait AgentTransport {
    fn agent_id(&self) -> &str;
    fn deliver_and_collect(&mut self, archive: &ArchiveBlob) -> Result<HostEvidence, TransportError>;
}

/// Fake SSH agent driven by fixture evidence + local hash check.
#[derive(Debug, Clone)]
pub struct FakeAgent {
    pub id: String,
    /// Template evidence (archive_sha256 overwritten on success).
    pub template: HostEvidence,
    /// If true, mutate archive bytes before "remote" verify → mismatch stop.
    pub corrupt_archive: bool,
    /// If true, report remote_archive_verified=false even when hash ok.
    pub force_remote_verify_fail: bool,
}

impl FakeAgent {
    pub fn from_evidence(template: HostEvidence) -> Self {
        Self {
            id: template.host_manifest.host_id.clone(),
            template,
            corrupt_archive: false,
            force_remote_verify_fail: false,
        }
    }

    pub fn load_fixture(path: &Path) -> Result<Self, TransportError> {
        let text = fs::read_to_string(path)?;
        let template = HostEvidence::from_json(&text)?;
        Ok(Self::from_evidence(template))
    }
}

impl AgentTransport for FakeAgent {
    fn agent_id(&self) -> &str {
        &self.id
    }

    fn deliver_and_collect(
        &mut self,
        archive: &ArchiveBlob,
    ) -> Result<HostEvidence, TransportError> {
        let mut remote_bytes = archive.bytes.clone();
        if self.corrupt_archive {
            if let Some(b) = remote_bytes.last_mut() {
                *b ^= 0xff;
            } else {
                remote_bytes.push(0);
            }
        }

        // Remote hash verification (agent side).
        let remote_check = verify_bytes_hash(&remote_bytes, &archive.sha256);
        match remote_check {
            Ok(_) => {
                if self.force_remote_verify_fail {
                    return Err(TransportError::Msg(format!(
                        "agent {}: remote hash verify forced fail",
                        self.id
                    )));
                }
                let mut evidence = self.template.clone();
                evidence.archive_sha256 = archive.sha256.clone();
                evidence.remote_archive_verified = true;
                Ok(evidence)
            }
            Err(e) => {
                // Hash mismatch stops before exec — no evidence with pass path.
                Err(TransportError::Msg(format!(
                    "agent {}: archive hash mismatch stops exec: {e}",
                    self.id
                )))
            }
        }
    }
}

/// Run fake matrix; stop agent on archive mismatch (no silent continue).
pub fn run_fake_matrix(
    archive: &ArchiveBlob,
    agents: &mut [FakeAgent],
) -> Result<Vec<HostEvidence>, TransportError> {
    let mut out = Vec::with_capacity(agents.len());
    for agent in agents.iter_mut() {
        out.push(agent.deliver_and_collect(archive)?);
    }
    Ok(out)
}

/// SSH transport placeholder — real remote path lands in runner tickets.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SshTransport {
    pub id: String,
    pub host: String,
}

impl AgentTransport for SshTransport {
    fn agent_id(&self) -> &str {
        &self.id
    }

    fn deliver_and_collect(
        &mut self,
        _archive: &ArchiveBlob,
    ) -> Result<HostEvidence, TransportError> {
        Err(TransportError::Msg(format!(
            "ssh transport for agent {} host {} not implemented (T15+)",
            self.id, self.host
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{build_archive, ArchiveEntry};
    use crate::report::{HostManifest, RawTrialSamples};
    use mmd_engine::bench::GATE_AGENT_COUNT;

    fn sample_evidence(id: &str) -> HostEvidence {
        let mut e = HostEvidence::new(
            HostManifest::new(id, "linux-x86_64", "vulkan", "test"),
            "pending",
        );
        e.submitted_frames = 10;
        e.completed_frames = 10;
        e.max_in_flight = 2;
        for i in 0..7 {
            e.raw_trials.push(RawTrialSamples {
                agent_count: GATE_AGENT_COUNT,
                trial_index: i,
                frame_service_ms: vec![12.0; 32],
            });
        }
        e
    }

    #[test]
    fn fake_agent_accepts_matching_archive() {
        let blob = build_archive(vec![ArchiveEntry {
            path: "a".into(),
            data: b"1".to_vec(),
        }])
        .unwrap();
        let mut agent = FakeAgent::from_evidence(sample_evidence("u"));
        let ev = agent.deliver_and_collect(&blob).unwrap();
        assert!(ev.remote_archive_verified);
        assert_eq!(ev.archive_sha256, blob.sha256);
    }

    #[test]
    fn corrupt_archive_stops() {
        let blob = build_archive(vec![ArchiveEntry {
            path: "a".into(),
            data: b"1".to_vec(),
        }])
        .unwrap();
        let mut agent = FakeAgent::from_evidence(sample_evidence("u"));
        agent.corrupt_archive = true;
        let err = agent.deliver_and_collect(&blob).unwrap_err();
        assert!(
            err.to_string().contains("hash mismatch"),
            "err={err}"
        );
    }
}
