//! Host evidence schemas. Candidate claims are untrusted input.

use serde::{Deserialize, Serialize};

/// Host identity / attestation surface (may be trusted via external path later).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostManifest {
    pub schema_version: String,
    pub host_id: String,
    /// linux-x86_64 | windows-x86_64 | macos-arm64
    pub platform: String,
    pub backend: String,
    pub os_build: String,
    #[serde(default)]
    pub cpu: String,
    #[serde(default)]
    pub gpu: String,
    #[serde(default)]
    pub attested: bool,
}

impl HostManifest {
    pub const SCHEMA: &'static str = "lab-host-manifest-v1";

    pub fn new(
        host_id: impl Into<String>,
        platform: impl Into<String>,
        backend: impl Into<String>,
        os_build: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: Self::SCHEMA.into(),
            host_id: host_id.into(),
            platform: platform.into(),
            backend: backend.into(),
            os_build: os_build.into(),
            cpu: String::new(),
            gpu: String::new(),
            attested: true,
        }
    }
}

/// Raw per-trial frame service samples (perf truth source).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawTrialSamples {
    pub agent_count: u32,
    pub trial_index: u32,
    pub frame_service_ms: Vec<f64>,
}

/// Untrusted candidate-reported summary (may be forged).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaimedStats {
    pub median_p95_ms: f64,
    pub median_p99_ms: f64,
    pub verdict: String,
}

/// Evidence bundle returned by one agent after archive delivery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostEvidence {
    pub schema_version: String,
    pub host_manifest: HostManifest,
    /// Archive digest the remote claims to have verified.
    pub archive_sha256: String,
    /// Remote hash verification result (agent-reported; coordinator also checks).
    pub remote_archive_verified: bool,
    /// Raw samples for coordinator recompute (required for perf truth).
    pub raw_trials: Vec<RawTrialSamples>,
    /// Optional forged/honest claimed stats from candidate report.
    #[serde(default)]
    pub claimed: Option<ClaimedStats>,
    #[serde(default)]
    pub project_rust_alloc_count: u64,
    #[serde(default)]
    pub submitted_frames: u64,
    #[serde(default)]
    pub completed_frames: u64,
    #[serde(default)]
    pub max_in_flight: usize,
}

impl HostEvidence {
    pub const SCHEMA: &'static str = "lab-host-evidence-v1";

    pub fn new(host_manifest: HostManifest, archive_sha256: impl Into<String>) -> Self {
        Self {
            schema_version: Self::SCHEMA.into(),
            host_manifest,
            archive_sha256: archive_sha256.into(),
            remote_archive_verified: true,
            raw_trials: Vec::new(),
            claimed: None,
            project_rust_alloc_count: 0,
            submitted_frames: 0,
            completed_frames: 0,
            max_in_flight: 2,
        }
    }

    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Lab config (example-compatible).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LabConfig {
    pub schema_version: String,
    pub agents: Vec<AgentConfig>,
    #[serde(default = "default_retain_days")]
    pub ordinary_retain_days: u32,
}

fn default_retain_days() -> u32 {
    90
}

impl LabConfig {
    #[allow(dead_code)]
    pub const SCHEMA: &'static str = "lab-config-v1";
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentConfig {
    pub id: String,
    pub platform: String,
    pub backend: String,
    /// ssh | fake
    pub transport: String,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub fixture: Option<String>,
}
