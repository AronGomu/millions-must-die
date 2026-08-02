//! macOS runner host-attestation contract.
//!
//! Separates frozen host/model/build/Metal/SSV/MDM/EACS pins from candidate
//! evidence (`lab-host-evidence-v1`). This module never accepts trial samples or
//! claimed perf stats — only host state.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MANIFEST_SCHEMA: &str = "macos-runner-manifest-v1";
pub const ATTESTATION_SCHEMA: &str = "macos-host-attestation-v1";

#[derive(Debug, Error)]
pub enum MacosContractError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("schema: {0}")]
    Schema(String),
}

/// Frozen expected macOS ref contract (checked into repo).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MacosRunnerManifest {
    pub schema_version: String,
    pub runner_id: String,
    pub platform: String,
    pub os_name: String,
    pub os_version: String,
    /// Exact macOS build pin (e.g. 24A335). Drift → maintenance-block.
    pub os_build: String,
    pub arch: String,
    pub backend: String,
    pub hardware: HardwareExpect,
    pub metal: MetalExpect,
    pub security: SecurityExpect,
    pub enrollment: EnrollmentExpect,
    pub eacs: EacsExpect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HardwareExpect {
    /// Marketing model substring (Mac mini).
    pub model_contains: String,
    /// Chip family pin (M4).
    pub chip_contains: String,
    pub memory_gb: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetalExpect {
    pub require_metal: bool,
    pub device_name_contains: String,
    #[serde(default)]
    pub reject_substrings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SecurityExpect {
    /// Expected Secure Enclave / System Integrity style mode label.
    pub security_mode: String,
    pub ssv_valid_required: bool,
    pub full_security_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnrollmentExpect {
    pub ade_required: bool,
    pub mdm_enrolled_required: bool,
    pub mdm_profile_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EacsExpect {
    /// Reset acknowledgment must be present before recovery-ready.
    pub reset_ack_required: bool,
    pub preflight_ok_required: bool,
}

/// Observed host attestation fields (from inspect script / fixture).
/// Not candidate evidence: no archive hash, trials, or claimed stats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MacosHostAttestation {
    pub schema_version: String,
    pub runner_id: String,
    pub platform: String,
    pub os_name: String,
    pub os_version: String,
    pub os_build: String,
    pub arch: String,
    pub backend: String,
    pub hardware_model: String,
    pub chip: String,
    pub memory_gb: u32,
    pub metal_available: bool,
    pub metal_device_name: String,
    pub security_mode: String,
    pub ssv_valid: bool,
    pub full_security: bool,
    pub ade_enrolled: bool,
    pub mdm_enrolled: bool,
    pub mdm_profile_id: String,
    pub eacs_preflight_ok: bool,
    pub eacs_reset_ack: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MacosAttestVerdict {
    /// Host matches contract; safe to proceed to recovery/candidate path later.
    ReadyForRecovery,
    /// Drift that requires quarantine + reflash/replace (SSV/reset/HW/MDM).
    Quarantine,
    /// OS build drift: scheduled maintenance, not emergency quarantine.
    MaintenanceBlock,
    /// Hard reject (wrong model/chip, software Metal, wrong platform).
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacosAttestResult {
    pub verdict: MacosAttestVerdict,
    pub reasons: Vec<String>,
}

impl MacosAttestResult {
    pub fn ok() -> Self {
        Self {
            verdict: MacosAttestVerdict::ReadyForRecovery,
            reasons: Vec::new(),
        }
    }
}

pub fn load_manifest(path: &Path) -> Result<MacosRunnerManifest, MacosContractError> {
    let text = fs::read_to_string(path)?;
    parse_manifest_toml(&text)
}

pub fn parse_manifest_toml(text: &str) -> Result<MacosRunnerManifest, MacosContractError> {
    let m: MacosRunnerManifest = toml::from_str(text)?;
    if m.schema_version != MANIFEST_SCHEMA {
        return Err(MacosContractError::Schema(format!(
            "expected {MANIFEST_SCHEMA}, got {}",
            m.schema_version
        )));
    }
    if m.hardware.memory_gb == 0 {
        return Err(MacosContractError::Schema(
            "hardware.memory_gb must be > 0".into(),
        ));
    }
    Ok(m)
}

pub fn load_attestation(path: &Path) -> Result<MacosHostAttestation, MacosContractError> {
    let text = fs::read_to_string(path)?;
    parse_attestation_json(&text)
}

pub fn parse_attestation_json(text: &str) -> Result<MacosHostAttestation, MacosContractError> {
    let a: MacosHostAttestation = serde_json::from_str(text)?;
    if a.schema_version != ATTESTATION_SCHEMA {
        return Err(MacosContractError::Schema(format!(
            "expected {ATTESTATION_SCHEMA}, got {}",
            a.schema_version
        )));
    }
    Ok(a)
}

/// Compare observed host attestation against frozen macOS runner contract.
pub fn validate_macos_attestation(
    expected: &MacosRunnerManifest,
    observed: &MacosHostAttestation,
) -> MacosAttestResult {
    let mut reject: Vec<String> = Vec::new();
    let mut quarantine: Vec<String> = Vec::new();
    let mut maintenance: Vec<String> = Vec::new();

    if observed.platform != expected.platform {
        reject.push(format!(
            "platform {} != {}",
            observed.platform, expected.platform
        ));
    }
    if observed.backend != expected.backend {
        reject.push(format!(
            "backend {} != {}",
            observed.backend, expected.backend
        ));
    }
    if observed.arch != expected.arch {
        reject.push(format!("arch {} != {}", observed.arch, expected.arch));
    }
    if observed.os_name != expected.os_name || observed.os_version != expected.os_version {
        quarantine.push(format!(
            "os {} {} != {} {}",
            observed.os_name, observed.os_version, expected.os_name, expected.os_version
        ));
    }
    if observed.runner_id != expected.runner_id {
        quarantine.push(format!(
            "runner_id {} != {}",
            observed.runner_id, expected.runner_id
        ));
    }

    // OS build drift is maintenance-block (scheduled pin refresh).
    if observed.os_build != expected.os_build {
        maintenance.push(format!(
            "os_build {} != {} (maintenance block)",
            observed.os_build, expected.os_build
        ));
    }

    // Wrong model / chip → hard reject (non-M4 / non Mac mini).
    let model_ok = observed
        .hardware_model
        .to_ascii_lowercase()
        .contains(&expected.hardware.model_contains.to_ascii_lowercase());
    let chip_ok = observed
        .chip
        .to_ascii_lowercase()
        .contains(&expected.hardware.chip_contains.to_ascii_lowercase());
    if !model_ok || !chip_ok {
        reject.push(format!(
            "wrong model/chip: model='{}' chip='{}' (need model~'{}' chip~'{}')",
            observed.hardware_model,
            observed.chip,
            expected.hardware.model_contains,
            expected.hardware.chip_contains
        ));
    }

    if observed.memory_gb != expected.hardware.memory_gb {
        quarantine.push(format!(
            "memory_gb {} != {}",
            observed.memory_gb, expected.hardware.memory_gb
        ));
    }

    // Metal: require real device; reject software/MoltenVK-style adapters.
    let device = observed.metal_device_name.to_ascii_lowercase();
    let hit_reject_sub = expected.metal.reject_substrings.iter().any(|s| {
        device.contains(&s.to_ascii_lowercase())
    });
    if expected.metal.require_metal && !observed.metal_available {
        reject.push("metal required but unavailable".into());
    } else if hit_reject_sub {
        reject.push(format!(
            "metal device rejected: '{}'",
            observed.metal_device_name
        ));
    } else if !device.contains(&expected.metal.device_name_contains.to_ascii_lowercase()) {
        quarantine.push(format!(
            "metal device '{}' missing '{}'",
            observed.metal_device_name, expected.metal.device_name_contains
        ));
    }

    // Full Security + SSV.
    if expected.security.full_security_required && !observed.full_security {
        quarantine.push("full_security required but not active".into());
    }
    if observed.security_mode != expected.security.security_mode {
        quarantine.push(format!(
            "security_mode '{}' != '{}'",
            observed.security_mode, expected.security.security_mode
        ));
    }
    if expected.security.ssv_valid_required && !observed.ssv_valid {
        quarantine.push("ssv invalid (seal failed)".into());
    }

    // ADE/MDM enrollment.
    if expected.enrollment.ade_required && !observed.ade_enrolled {
        quarantine.push("ade enrollment missing".into());
    }
    if expected.enrollment.mdm_enrolled_required && !observed.mdm_enrolled {
        quarantine.push("mdm enrollment missing".into());
    }
    if observed.mdm_enrolled
        && observed.mdm_profile_id != expected.enrollment.mdm_profile_id
    {
        quarantine.push(format!(
            "mdm_profile_id '{}' != '{}'",
            observed.mdm_profile_id, expected.enrollment.mdm_profile_id
        ));
    }

    // EACS preflight + reset ack. Missed reset ack = quarantine.
    if expected.eacs.preflight_ok_required && !observed.eacs_preflight_ok {
        quarantine.push("eacs preflight not ok".into());
    }
    if expected.eacs.reset_ack_required && !observed.eacs_reset_ack {
        quarantine.push("eacs reset ack missing (quarantine)".into());
    }

    // Precedence: reject > quarantine > maintenance-block > ready.
    if !reject.is_empty() {
        return MacosAttestResult {
            verdict: MacosAttestVerdict::Reject,
            reasons: reject,
        };
    }
    if !quarantine.is_empty() {
        return MacosAttestResult {
            verdict: MacosAttestVerdict::Quarantine,
            reasons: quarantine,
        };
    }
    if !maintenance.is_empty() {
        return MacosAttestResult {
            verdict: MacosAttestVerdict::MaintenanceBlock,
            reasons: maintenance,
        };
    }
    MacosAttestResult::ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> MacosRunnerManifest {
        parse_manifest_toml(
            r#"
schema_version = "macos-runner-manifest-v1"
runner_id = "macos-ref"
platform = "macos-arm64"
os_name = "macOS"
os_version = "15"
os_build = "24A335"
arch = "arm64"
backend = "metal"
[hardware]
model_contains = "Mac mini"
chip_contains = "M4"
memory_gb = 16
[metal]
require_metal = true
device_name_contains = "Apple M4"
reject_substrings = ["MoltenVK", "llvmpipe", "SwiftShader"]
[security]
security_mode = "Full Security"
ssv_valid_required = true
full_security_required = true
[enrollment]
ade_required = true
mdm_enrolled_required = true
mdm_profile_id = "mmd-lab-macos-ref"
[eacs]
reset_ack_required = true
preflight_ok_required = true
"#,
        )
        .unwrap()
    }

    fn sample_pass() -> MacosHostAttestation {
        parse_attestation_json(
            r#"{
  "schema_version": "macos-host-attestation-v1",
  "runner_id": "macos-ref",
  "platform": "macos-arm64",
  "os_name": "macOS",
  "os_version": "15",
  "os_build": "24A335",
  "arch": "arm64",
  "backend": "metal",
  "hardware_model": "Mac mini (2024)",
  "chip": "Apple M4",
  "memory_gb": 16,
  "metal_available": true,
  "metal_device_name": "Apple M4",
  "security_mode": "Full Security",
  "ssv_valid": true,
  "full_security": true,
  "ade_enrolled": true,
  "mdm_enrolled": true,
  "mdm_profile_id": "mmd-lab-macos-ref",
  "eacs_preflight_ok": true,
  "eacs_reset_ack": true
}"#,
        )
        .unwrap()
    }

    #[test]
    fn macos_manifest_unit_pass() {
        let r = validate_macos_attestation(&sample_manifest(), &sample_pass());
        assert_eq!(r.verdict, MacosAttestVerdict::ReadyForRecovery, "{r:?}");
    }

    #[test]
    fn macos_manifest_unit_wrong_model_reject() {
        let mut o = sample_pass();
        o.hardware_model = "MacBook Pro (16-inch, 2023)".into();
        o.chip = "Apple M3 Pro".into();
        let r = validate_macos_attestation(&sample_manifest(), &o);
        assert_eq!(r.verdict, MacosAttestVerdict::Reject);
    }

    #[test]
    fn macos_manifest_unit_invalid_ssv_quarantine() {
        let mut o = sample_pass();
        o.ssv_valid = false;
        let r = validate_macos_attestation(&sample_manifest(), &o);
        assert_eq!(r.verdict, MacosAttestVerdict::Quarantine);
        assert!(r.reasons.iter().any(|s| s.contains("ssv")));
    }

    #[test]
    fn macos_manifest_unit_missing_mdm_quarantine() {
        let mut o = sample_pass();
        o.mdm_enrolled = false;
        let r = validate_macos_attestation(&sample_manifest(), &o);
        assert_eq!(r.verdict, MacosAttestVerdict::Quarantine);
        assert!(r.reasons.iter().any(|s| s.contains("mdm")));
    }

    #[test]
    fn macos_manifest_unit_missed_reset_ack_quarantine() {
        let mut o = sample_pass();
        o.eacs_reset_ack = false;
        let r = validate_macos_attestation(&sample_manifest(), &o);
        assert_eq!(r.verdict, MacosAttestVerdict::Quarantine);
        assert!(r.reasons.iter().any(|s| s.contains("reset ack")));
    }
}
