//! Ubuntu runner host-attestation contract.
//!
//! Separates frozen host/image/firmware/Vulkan pins from candidate evidence
//! (`lab-host-evidence-v1`). This module never accepts trial samples or claimed
//! perf stats — only host state.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MANIFEST_SCHEMA: &str = "ubuntu-runner-manifest-v1";
pub const ATTESTATION_SCHEMA: &str = "ubuntu-host-attestation-v1";

#[derive(Debug, Error)]
pub enum UbuntuContractError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("schema: {0}")]
    Schema(String),
}

/// Frozen expected Ubuntu ref contract (checked into repo).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UbuntuRunnerManifest {
    pub schema_version: String,
    pub runner_id: String,
    pub platform: String,
    pub os_name: String,
    pub os_version: String,
    pub arch: String,
    pub backend: String,
    pub cpu: CpuExpect,
    pub gpu: GpuExpect,
    pub driver: DriverExpect,
    pub firmware: FirmwareExpect,
    pub image: ImageExpect,
    pub vulkan: VulkanExpect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CpuExpect {
    pub model_contains: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuExpect {
    pub model_contains: String,
    pub vram_mb: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DriverExpect {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FirmwareExpect {
    pub bios_version: String,
    pub vbios_version: String,
    pub secure_boot_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageExpect {
    pub digest_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VulkanExpect {
    pub allow_software: bool,
    pub device_name_contains: String,
    #[serde(default)]
    pub reject_substrings: Vec<String>,
}

/// Observed host attestation fields (from inspect script / fixture).
/// Not candidate evidence: no archive hash, trials, or claimed stats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UbuntuHostAttestation {
    pub schema_version: String,
    pub runner_id: String,
    pub platform: String,
    pub os_name: String,
    pub os_version: String,
    pub arch: String,
    pub backend: String,
    pub cpu_model: String,
    pub gpu_model: String,
    pub gpu_vram_mb: u32,
    pub driver_name: String,
    pub driver_version: String,
    pub bios_version: String,
    pub vbios_version: String,
    pub secure_boot: bool,
    pub image_digest_sha256: String,
    pub vulkan_device_name: String,
    pub vulkan_device_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AttestVerdict {
    /// Host matches contract; safe to proceed to recovery/candidate path later.
    ReadyForRecovery,
    /// Drift that requires quarantine + reflash/replace (image/firmware/HW).
    Quarantine,
    /// Hard reject (software Vulkan, wrong backend/platform).
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestResult {
    pub verdict: AttestVerdict,
    pub reasons: Vec<String>,
}

impl AttestResult {
    pub fn ok() -> Self {
        Self {
            verdict: AttestVerdict::ReadyForRecovery,
            reasons: Vec::new(),
        }
    }

}

pub fn load_manifest(path: &Path) -> Result<UbuntuRunnerManifest, UbuntuContractError> {
    let text = fs::read_to_string(path)?;
    parse_manifest_toml(&text)
}

pub fn parse_manifest_toml(text: &str) -> Result<UbuntuRunnerManifest, UbuntuContractError> {
    let m: UbuntuRunnerManifest = toml::from_str(text)?;
    if m.schema_version != MANIFEST_SCHEMA {
        return Err(UbuntuContractError::Schema(format!(
            "expected {MANIFEST_SCHEMA}, got {}",
            m.schema_version
        )));
    }
    if m.image.digest_sha256.len() != 64
        || !m
            .image
            .digest_sha256
            .chars()
            .all(|c| c.is_ascii_hexdigit())
    {
        return Err(UbuntuContractError::Schema(
            "image.digest_sha256 must be 64 hex chars".into(),
        ));
    }
    Ok(m)
}

pub fn load_attestation(path: &Path) -> Result<UbuntuHostAttestation, UbuntuContractError> {
    let text = fs::read_to_string(path)?;
    parse_attestation_json(&text)
}

pub fn parse_attestation_json(text: &str) -> Result<UbuntuHostAttestation, UbuntuContractError> {
    let a: UbuntuHostAttestation = serde_json::from_str(text)?;
    if a.schema_version != ATTESTATION_SCHEMA {
        return Err(UbuntuContractError::Schema(format!(
            "expected {ATTESTATION_SCHEMA}, got {}",
            a.schema_version
        )));
    }
    Ok(a)
}

/// Compare observed host attestation against frozen Ubuntu runner contract.
pub fn validate_ubuntu_attestation(
    expected: &UbuntuRunnerManifest,
    observed: &UbuntuHostAttestation,
) -> AttestResult {
    let mut reject: Vec<String> = Vec::new();
    let mut quarantine: Vec<String> = Vec::new();

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

    // Software / CPU Vulkan adapters are never accepted on the Ubuntu ref.
    let dev = observed.vulkan_device_name.to_ascii_lowercase();
    let dtype = observed.vulkan_device_type.to_ascii_lowercase();
    let software_type = matches!(dtype.as_str(), "cpu" | "software" | "virtual");
    let hit_reject_sub = expected
        .vulkan
        .reject_substrings
        .iter()
        .any(|s| dev.contains(&s.to_ascii_lowercase()));
    if !expected.vulkan.allow_software && (software_type || hit_reject_sub) {
        reject.push(format!(
            "software vulkan rejected: device='{}' type='{}'",
            observed.vulkan_device_name, observed.vulkan_device_type
        ));
    } else if !dev.contains(&expected.vulkan.device_name_contains.to_ascii_lowercase()) {
        quarantine.push(format!(
            "vulkan device '{}' missing '{}'",
            observed.vulkan_device_name, expected.vulkan.device_name_contains
        ));
    }

    if !observed
        .cpu_model
        .to_ascii_lowercase()
        .contains(&expected.cpu.model_contains.to_ascii_lowercase())
    {
        quarantine.push(format!(
            "cpu '{}' missing '{}'",
            observed.cpu_model, expected.cpu.model_contains
        ));
    }

    // GPU model check skipped when already rejected as software adapter.
    if reject.is_empty()
        && !observed
            .gpu_model
            .to_ascii_lowercase()
            .contains(&expected.gpu.model_contains.to_ascii_lowercase())
    {
        quarantine.push(format!(
            "gpu '{}' missing '{}'",
            observed.gpu_model, expected.gpu.model_contains
        ));
    }

    if observed.gpu_vram_mb != expected.gpu.vram_mb {
        quarantine.push(format!(
            "gpu_vram_mb {} != {}",
            observed.gpu_vram_mb, expected.gpu.vram_mb
        ));
    }

    if observed.driver_name != expected.driver.name
        || observed.driver_version != expected.driver.version
    {
        quarantine.push(format!(
            "driver {} {} != {} {}",
            observed.driver_name,
            observed.driver_version,
            expected.driver.name,
            expected.driver.version
        ));
    }

    if observed.bios_version != expected.firmware.bios_version {
        quarantine.push(format!(
            "bios {} != {}",
            observed.bios_version, expected.firmware.bios_version
        ));
    }
    if observed.vbios_version != expected.firmware.vbios_version {
        quarantine.push(format!(
            "vbios {} != {}",
            observed.vbios_version, expected.firmware.vbios_version
        ));
    }
    if expected.firmware.secure_boot_required && !observed.secure_boot {
        quarantine.push("secure_boot required but disabled".into());
    }

    let obs_digest = observed.image_digest_sha256.to_ascii_lowercase();
    let exp_digest = expected.image.digest_sha256.to_ascii_lowercase();
    if obs_digest != exp_digest {
        quarantine.push(format!(
            "image digest {} != {}",
            observed.image_digest_sha256, expected.image.digest_sha256
        ));
    }

    if !reject.is_empty() {
        return AttestResult {
            verdict: AttestVerdict::Reject,
            reasons: reject,
        };
    }
    if !quarantine.is_empty() {
        return AttestResult {
            verdict: AttestVerdict::Quarantine,
            reasons: quarantine,
        };
    }
    AttestResult::ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> UbuntuRunnerManifest {
        parse_manifest_toml(
            r#"
schema_version = "ubuntu-runner-manifest-v1"
runner_id = "ubuntu-ref"
platform = "linux-x86_64"
os_name = "Ubuntu"
os_version = "24.04"
arch = "x86_64"
backend = "vulkan"
[cpu]
model_contains = "8600G"
[gpu]
model_contains = "RX 6400"
vram_mb = 4096
[driver]
name = "amdgpu"
version = "23.2.1"
[firmware]
bios_version = "F1a"
vbios_version = "113-D5040100-100"
secure_boot_required = true
[image]
digest_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
[vulkan]
allow_software = false
device_name_contains = "RX 6400"
reject_substrings = ["llvmpipe", "lavapipe"]
"#,
        )
        .unwrap()
    }

    fn sample_pass() -> UbuntuHostAttestation {
        parse_attestation_json(
            r#"{
  "schema_version": "ubuntu-host-attestation-v1",
  "runner_id": "ubuntu-ref",
  "platform": "linux-x86_64",
  "os_name": "Ubuntu",
  "os_version": "24.04",
  "arch": "x86_64",
  "backend": "vulkan",
  "cpu_model": "AMD Ryzen 5 8600G",
  "gpu_model": "AMD Radeon RX 6400",
  "gpu_vram_mb": 4096,
  "driver_name": "amdgpu",
  "driver_version": "23.2.1",
  "bios_version": "F1a",
  "vbios_version": "113-D5040100-100",
  "secure_boot": true,
  "image_digest_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "vulkan_device_name": "AMD Radeon RX 6400",
  "vulkan_device_type": "discrete_gpu"
}"#,
        )
        .unwrap()
    }

    #[test]
    fn ubuntu_manifest_unit_pass() {
        let r = validate_ubuntu_attestation(&sample_manifest(), &sample_pass());
        assert_eq!(r.verdict, AttestVerdict::ReadyForRecovery, "{r:?}");
    }

    #[test]
    fn ubuntu_manifest_unit_llvmpipe_reject() {
        let mut o = sample_pass();
        o.vulkan_device_name = "llvmpipe (LLVM)".into();
        o.vulkan_device_type = "cpu".into();
        o.gpu_model = "llvmpipe".into();
        let r = validate_ubuntu_attestation(&sample_manifest(), &o);
        assert_eq!(r.verdict, AttestVerdict::Reject);
    }
}
