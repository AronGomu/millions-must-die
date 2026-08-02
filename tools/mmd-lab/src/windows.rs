//! Windows runner host-attestation contract.
//!
//! Separates frozen host/FFU/driver/power/D3D12 pins from candidate evidence
//! (`lab-host-evidence-v1`). This module never accepts trial samples or claimed
//! perf stats — only host state.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MANIFEST_SCHEMA: &str = "windows-runner-manifest-v1";
pub const ATTESTATION_SCHEMA: &str = "windows-host-attestation-v1";

#[derive(Debug, Error)]
pub enum WindowsContractError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("schema: {0}")]
    Schema(String),
}

/// Frozen expected Windows ref contract (checked into repo).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowsRunnerManifest {
    pub schema_version: String,
    pub runner_id: String,
    pub platform: String,
    pub os_name: String,
    pub os_version: String,
    /// Exact Windows display/build string (e.g. 25H2 build pin). Drift → maintenance-block.
    pub os_build: String,
    pub arch: String,
    pub backend: String,
    pub cpu: CpuExpect,
    pub gpu: GpuExpect,
    pub driver: DriverExpect,
    pub firmware: FirmwareExpect,
    pub ffu: FfuExpect,
    pub power: PowerExpect,
    pub d3d12: D3d12Expect,
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
    pub measured_boot_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FfuExpect {
    /// SHA-256 of frozen WinPE/FFU whole-disk image (placeholder until T19).
    pub digest_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PowerExpect {
    /// Exact active power-plan name pin.
    pub plan_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct D3d12Expect {
    pub allow_basic_render: bool,
    pub device_name_contains: String,
    #[serde(default)]
    pub reject_substrings: Vec<String>,
}

/// Observed host attestation fields (from inspect script / fixture).
/// Not candidate evidence: no archive hash, trials, or claimed stats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowsHostAttestation {
    pub schema_version: String,
    pub runner_id: String,
    pub platform: String,
    pub os_name: String,
    pub os_version: String,
    pub os_build: String,
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
    pub measured_boot: bool,
    pub ffu_digest_sha256: String,
    pub power_plan_name: String,
    pub d3d12_adapter_name: String,
    pub d3d12_adapter_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowsAttestVerdict {
    /// Host matches contract; safe to proceed to recovery/candidate path later.
    ReadyForRecovery,
    /// Drift that requires quarantine + reflash/replace (FFU/firmware/HW).
    Quarantine,
    /// OS build drift: scheduled maintenance, not emergency quarantine.
    MaintenanceBlock,
    /// Hard reject (Basic Render Driver, wrong backend/platform).
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowsAttestResult {
    pub verdict: WindowsAttestVerdict,
    pub reasons: Vec<String>,
}

impl WindowsAttestResult {
    pub fn ok() -> Self {
        Self {
            verdict: WindowsAttestVerdict::ReadyForRecovery,
            reasons: Vec::new(),
        }
    }
}

pub fn load_manifest(path: &Path) -> Result<WindowsRunnerManifest, WindowsContractError> {
    let text = fs::read_to_string(path)?;
    parse_manifest_toml(&text)
}

pub fn parse_manifest_toml(text: &str) -> Result<WindowsRunnerManifest, WindowsContractError> {
    let m: WindowsRunnerManifest = toml::from_str(text)?;
    if m.schema_version != MANIFEST_SCHEMA {
        return Err(WindowsContractError::Schema(format!(
            "expected {MANIFEST_SCHEMA}, got {}",
            m.schema_version
        )));
    }
    if m.ffu.digest_sha256.len() != 64
        || !m.ffu.digest_sha256.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(WindowsContractError::Schema(
            "ffu.digest_sha256 must be 64 hex chars".into(),
        ));
    }
    Ok(m)
}

pub fn load_attestation(path: &Path) -> Result<WindowsHostAttestation, WindowsContractError> {
    let text = fs::read_to_string(path)?;
    parse_attestation_json(&text)
}

pub fn parse_attestation_json(text: &str) -> Result<WindowsHostAttestation, WindowsContractError> {
    let a: WindowsHostAttestation = serde_json::from_str(text)?;
    if a.schema_version != ATTESTATION_SCHEMA {
        return Err(WindowsContractError::Schema(format!(
            "expected {ATTESTATION_SCHEMA}, got {}",
            a.schema_version
        )));
    }
    Ok(a)
}

/// Compare observed host attestation against frozen Windows runner contract.
pub fn validate_windows_attestation(
    expected: &WindowsRunnerManifest,
    observed: &WindowsHostAttestation,
) -> WindowsAttestResult {
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

    // OS build drift is maintenance-block (scheduled pin refresh), not quarantine.
    if observed.os_build != expected.os_build {
        maintenance.push(format!(
            "os_build {} != {} (maintenance block)",
            observed.os_build, expected.os_build
        ));
    }

    // Microsoft Basic Render Driver / WARP never accepted on Windows ref.
    let adapter = observed.d3d12_adapter_name.to_ascii_lowercase();
    let atype = observed.d3d12_adapter_type.to_ascii_lowercase();
    let software_type = matches!(
        atype.as_str(),
        "basic" | "software" | "warp" | "cpu" | "microsoft basic render driver"
    );
    let hit_reject_sub = expected.d3d12.reject_substrings.iter().any(|s| {
        adapter.contains(&s.to_ascii_lowercase())
            || observed
                .gpu_model
                .to_ascii_lowercase()
                .contains(&s.to_ascii_lowercase())
    });
    if !expected.d3d12.allow_basic_render && (software_type || hit_reject_sub) {
        reject.push(format!(
            "basic render driver rejected: adapter='{}' type='{}'",
            observed.d3d12_adapter_name, observed.d3d12_adapter_type
        ));
    } else if !adapter.contains(&expected.d3d12.device_name_contains.to_ascii_lowercase()) {
        quarantine.push(format!(
            "d3d12 adapter '{}' missing '{}'",
            observed.d3d12_adapter_name, expected.d3d12.device_name_contains
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
    if expected.firmware.measured_boot_required && !observed.measured_boot {
        quarantine.push("measured_boot required but disabled/unsupported".into());
    }

    let obs_digest = observed.ffu_digest_sha256.to_ascii_lowercase();
    let exp_digest = expected.ffu.digest_sha256.to_ascii_lowercase();
    if obs_digest != exp_digest {
        quarantine.push(format!(
            "ffu digest {} != {}",
            observed.ffu_digest_sha256, expected.ffu.digest_sha256
        ));
    }

    if observed.power_plan_name != expected.power.plan_name {
        quarantine.push(format!(
            "power_plan '{}' != '{}'",
            observed.power_plan_name, expected.power.plan_name
        ));
    }

    // Precedence: reject > quarantine > maintenance-block > ready.
    if !reject.is_empty() {
        return WindowsAttestResult {
            verdict: WindowsAttestVerdict::Reject,
            reasons: reject,
        };
    }
    if !quarantine.is_empty() {
        return WindowsAttestResult {
            verdict: WindowsAttestVerdict::Quarantine,
            reasons: quarantine,
        };
    }
    if !maintenance.is_empty() {
        return WindowsAttestResult {
            verdict: WindowsAttestVerdict::MaintenanceBlock,
            reasons: maintenance,
        };
    }
    WindowsAttestResult::ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> WindowsRunnerManifest {
        parse_manifest_toml(
            r#"
schema_version = "windows-runner-manifest-v1"
runner_id = "windows-ref"
platform = "windows-x86_64"
os_name = "Windows"
os_version = "11"
os_build = "26200.6584"
arch = "x86_64"
backend = "d3d12"
[cpu]
model_contains = "8600G"
[gpu]
model_contains = "RX 6400"
vram_mb = 4096
[driver]
name = "AMD Adrenalin"
version = "24.12.1"
[firmware]
bios_version = "F1a"
vbios_version = "113-D5040100-100"
secure_boot_required = true
measured_boot_required = true
[ffu]
digest_sha256 = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
[power]
plan_name = "MMD High Performance"
[d3d12]
allow_basic_render = false
device_name_contains = "RX 6400"
reject_substrings = ["Microsoft Basic Render Driver", "Basic Render Driver", "WARP"]
"#,
        )
        .unwrap()
    }

    fn sample_pass() -> WindowsHostAttestation {
        parse_attestation_json(
            r#"{
  "schema_version": "windows-host-attestation-v1",
  "runner_id": "windows-ref",
  "platform": "windows-x86_64",
  "os_name": "Windows",
  "os_version": "11",
  "os_build": "26200.6584",
  "arch": "x86_64",
  "backend": "d3d12",
  "cpu_model": "AMD Ryzen 5 8600G",
  "gpu_model": "AMD Radeon RX 6400",
  "gpu_vram_mb": 4096,
  "driver_name": "AMD Adrenalin",
  "driver_version": "24.12.1",
  "bios_version": "F1a",
  "vbios_version": "113-D5040100-100",
  "secure_boot": true,
  "measured_boot": true,
  "ffu_digest_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
  "power_plan_name": "MMD High Performance",
  "d3d12_adapter_name": "AMD Radeon RX 6400",
  "d3d12_adapter_type": "discrete_gpu"
}"#,
        )
        .unwrap()
    }

    #[test]
    fn windows_manifest_unit_pass() {
        let r = validate_windows_attestation(&sample_manifest(), &sample_pass());
        assert_eq!(r.verdict, WindowsAttestVerdict::ReadyForRecovery, "{r:?}");
    }

    #[test]
    fn windows_manifest_unit_basic_render_reject() {
        let mut o = sample_pass();
        o.d3d12_adapter_name = "Microsoft Basic Render Driver".into();
        o.d3d12_adapter_type = "basic".into();
        o.gpu_model = "Microsoft Basic Render Driver".into();
        let r = validate_windows_attestation(&sample_manifest(), &o);
        assert_eq!(r.verdict, WindowsAttestVerdict::Reject);
    }

    #[test]
    fn windows_manifest_unit_build_drift_maintenance() {
        let mut o = sample_pass();
        o.os_build = "26100.0001".into();
        let r = validate_windows_attestation(&sample_manifest(), &o);
        assert_eq!(r.verdict, WindowsAttestVerdict::MaintenanceBlock);
    }

    #[test]
    fn windows_manifest_unit_wrong_ffu_quarantine() {
        let mut o = sample_pass();
        o.ffu_digest_sha256 =
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".into();
        let r = validate_windows_attestation(&sample_manifest(), &o);
        assert_eq!(r.verdict, WindowsAttestVerdict::Quarantine);
    }
}
