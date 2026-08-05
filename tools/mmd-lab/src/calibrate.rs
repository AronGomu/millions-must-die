//! Relative calibration engine. Separate from data collection.
//!
//! Consumes ≥50 clean same-manifest reports/platform, computes noise envelope,
//! accepts explicit reviewed margin only when margin > noise, emits disabled
//! baseline candidates. Never auto-enables.

use std::fs;
use std::path::Path;

use mmd_engine::bench::{NMAD_LIMIT, median, normalized_mad};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Minimum clean reports per platform/manifest.
pub const MIN_CLEAN_REPORTS: usize = 50;

pub const BASELINE_SCHEMA: &str = "baseline-v1";
pub const DATASET_SCHEMA: &str = "calibration-dataset-v1";

/// Metric keys tracked for relative baselines (ADR 005).
pub const METRIC_KEYS: [&str; 5] = [
    "median_p95_frame_service_ms",
    "median_p99_frame_service_ms",
    "median_sim_ms",
    "median_upload_ms",
    "median_gpu_queue_latency_ms",
];

#[derive(Debug, Error, PartialEq)]
pub enum CalibrateError {
    #[error("dataset schema_version must be {DATASET_SCHEMA}, got {0}")]
    BadDatasetSchema(String),
    #[error("baseline schema_version must be {BASELINE_SCHEMA}, got {0}")]
    BadBaselineSchema(String),
    #[error("need ≥{MIN_CLEAN_REPORTS} clean reports/platform/manifest; got {0}")]
    Underfilled(usize),
    #[error("mixed manifests reject: {0}")]
    MixedManifest(String),
    #[error("noisy calibration set: {metric} nmad={nmad:.6} > limit {limit}")]
    Noisy {
        metric: String,
        nmad: f64,
        limit: f64,
    },
    #[error("margin must exceed noise: metric={metric} margin={margin} noise_nmad={noise_nmad}")]
    MarginBelowNoise {
        metric: String,
        margin: f64,
        noise_nmad: f64,
    },
    #[error("margin must be finite and > 0; got {0}")]
    BadMargin(f64),
    #[error("candidate already enabled; refuse silent re-enable")]
    AlreadyEnabled,
    #[error("provenance '{0}' cannot enable baseline; only native reviewed pilot data may enable")]
    NonNativeProvenance(String),
    #[error("enabled baseline requires review record (reviewer + evidence_ref)")]
    MissingReview,
    #[error("enabled baseline must set review_required=false")]
    ReviewFlagInconsistent,
    #[error("disabled baseline must keep review_required=true and review=null")]
    CandidateReviewInconsistent,
    #[error("io: {0}")]
    Io(String),
    #[error("json: {0}")]
    Json(String),
}

/// Data provenance for calibration datasets and baselines.
///
/// Hardware deferral policy (2026-08-05): only `native` data collected on
/// attested lab hardware may ever enable a baseline. `synthetic` proves the
/// pipeline shape only; `unspecified` covers pre-provenance files. Both are
/// permanently refused by `enable_reviewed_baseline`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provenance {
    /// Real lane runs on attested reference hardware.
    Native,
    /// Generated data; pipeline proof only. Can never enable.
    Synthetic,
    /// Legacy/unmarked data; treated as not enableable.
    #[default]
    Unspecified,
}

impl Provenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Provenance::Native => "native",
            Provenance::Synthetic => "synthetic",
            Provenance::Unspecified => "unspecified",
        }
    }
}

/// Frozen host/workload pins bound to a baseline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalibrationManifest {
    pub scenario_version: String,
    pub scenario_sha256: String,
    pub shader_manifest_version: u32,
    pub os_build: String,
    #[serde(default)]
    pub driver: String,
    #[serde(default)]
    pub cpu: String,
    #[serde(default)]
    pub gpu: String,
    pub policy_id: String,
}

impl CalibrationManifest {
    /// Stable fingerprint for equality (manifest drift → reject).
    pub fn fingerprint(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.scenario_version,
            self.scenario_sha256,
            self.shader_manifest_version,
            self.os_build,
            self.driver,
            self.cpu,
            self.gpu,
            self.policy_id
        )
    }
}

/// One clean calibration run summary (from full report or synthetic).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationSample {
    pub run_id: String,
    pub median_p95_frame_service_ms: f64,
    pub median_p99_frame_service_ms: f64,
    pub median_sim_ms: f64,
    pub median_upload_ms: f64,
    pub median_gpu_queue_latency_ms: f64,
    /// Optional per-sample manifest; must match dataset when present.
    #[serde(default)]
    pub manifest_override: Option<CalibrationManifest>,
}

impl CalibrationSample {
    fn metric(&self, key: &str) -> f64 {
        match key {
            "median_p95_frame_service_ms" => self.median_p95_frame_service_ms,
            "median_p99_frame_service_ms" => self.median_p99_frame_service_ms,
            "median_sim_ms" => self.median_sim_ms,
            "median_upload_ms" => self.median_upload_ms,
            "median_gpu_queue_latency_ms" => self.median_gpu_queue_latency_ms,
            _ => f64::NAN,
        }
    }
}

/// Input dataset for relative calibration (collection lives elsewhere).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationDataset {
    pub schema_version: String,
    pub platform: String,
    pub backend: String,
    pub manifest: CalibrationManifest,
    /// Data origin; only `native` datasets may ever be enabled downstream.
    #[serde(default)]
    pub provenance: Provenance,
    pub samples: Vec<CalibrationSample>,
}

impl CalibrationDataset {
    pub fn from_json(s: &str) -> Result<Self, CalibrateError> {
        let ds: Self = serde_json::from_str(s).map_err(|e| CalibrateError::Json(e.to_string()))?;
        if ds.schema_version != DATASET_SCHEMA {
            return Err(CalibrateError::BadDatasetSchema(ds.schema_version));
        }
        Ok(ds)
    }

    pub fn load(path: &Path) -> Result<Self, CalibrateError> {
        let s = fs::read_to_string(path).map_err(|e| CalibrateError::Io(e.to_string()))?;
        Self::from_json(&s)
    }
}

/// Per-metric noise envelope + explicit margin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricBaseline {
    pub baseline_ms: f64,
    pub noise_nmad: f64,
    pub noise_envelope_ms: f64,
    /// Explicit relative margin (fraction). Reviewed value; never invented silently.
    pub margin: f64,
    pub relative_limit_ms: f64,
}

/// Owner review record required before enable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewRecord {
    pub reviewer: String,
    /// ISO-8601 UTC.
    pub reviewed_at: String,
    pub evidence_ref: String,
    #[serde(default)]
    pub notes: String,
}

/// Human-reviewable baseline candidate / enabled baseline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Baseline {
    pub schema_version: String,
    /// Always false on derive. Only enable-reviewed may set true.
    pub enabled: bool,
    pub review_required: bool,
    pub platform: String,
    pub backend: String,
    pub manifest: CalibrationManifest,
    /// Inherited from the source dataset; non-`native` can never enable.
    #[serde(default)]
    pub provenance: Provenance,
    pub sample_count: usize,
    pub nmad_limit: f64,
    pub metrics: BaselineMetrics,
    pub review: Option<ReviewRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BaselineMetrics {
    pub median_p95_frame_service_ms: MetricBaseline,
    pub median_p99_frame_service_ms: MetricBaseline,
    pub median_sim_ms: MetricBaseline,
    pub median_upload_ms: MetricBaseline,
    pub median_gpu_queue_latency_ms: MetricBaseline,
}

impl BaselineMetrics {
    fn get(&self, key: &str) -> Option<&MetricBaseline> {
        match key {
            "median_p95_frame_service_ms" => Some(&self.median_p95_frame_service_ms),
            "median_p99_frame_service_ms" => Some(&self.median_p99_frame_service_ms),
            "median_sim_ms" => Some(&self.median_sim_ms),
            "median_upload_ms" => Some(&self.median_upload_ms),
            "median_gpu_queue_latency_ms" => Some(&self.median_gpu_queue_latency_ms),
            _ => None,
        }
    }
}

impl Baseline {
    pub fn from_json(s: &str) -> Result<Self, CalibrateError> {
        let b: Self = serde_json::from_str(s).map_err(|e| CalibrateError::Json(e.to_string()))?;
        if b.schema_version != BASELINE_SCHEMA {
            return Err(CalibrateError::BadBaselineSchema(b.schema_version));
        }
        Ok(b)
    }

    pub fn load(path: &Path) -> Result<Self, CalibrateError> {
        let s = fs::read_to_string(path).map_err(|e| CalibrateError::Io(e.to_string()))?;
        Self::from_json(&s)
    }

    pub fn to_json_pretty(&self) -> Result<String, CalibrateError> {
        serde_json::to_string_pretty(self).map_err(|e| CalibrateError::Json(e.to_string()))
    }

    pub fn write(&self, path: &Path) -> Result<(), CalibrateError> {
        let s = self.to_json_pretty()?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| CalibrateError::Io(e.to_string()))?;
            }
        }
        fs::write(path, s).map_err(|e| CalibrateError::Io(e.to_string()))
    }

    /// Structural invariants for candidates + enabled baselines.
    pub fn validate_flags(&self) -> Result<(), CalibrateError> {
        if self.enabled {
            if self.review.is_none() {
                return Err(CalibrateError::MissingReview);
            }
            if self.review_required {
                return Err(CalibrateError::ReviewFlagInconsistent);
            }
        } else {
            if !self.review_required || self.review.is_some() {
                return Err(CalibrateError::CandidateReviewInconsistent);
            }
        }
        Ok(())
    }
}

/// Derive disabled baseline candidate from dataset + explicit margin.
pub fn derive_baseline_candidate(
    dataset: &CalibrationDataset,
    margin: f64,
) -> Result<Baseline, CalibrateError> {
    if !margin.is_finite() || margin <= 0.0 {
        return Err(CalibrateError::BadMargin(margin));
    }

    let n = dataset.samples.len();
    if n < MIN_CLEAN_REPORTS {
        return Err(CalibrateError::Underfilled(n));
    }

    let expected_fp = dataset.manifest.fingerprint();
    for s in &dataset.samples {
        if let Some(ref m) = s.manifest_override {
            if m.fingerprint() != expected_fp {
                return Err(CalibrateError::MixedManifest(format!(
                    "sample {} fp={} expected {}",
                    s.run_id,
                    m.fingerprint(),
                    expected_fp
                )));
            }
        }
    }

    let mut built = Vec::with_capacity(METRIC_KEYS.len());
    for key in METRIC_KEYS {
        let values: Vec<f64> = dataset.samples.iter().map(|s| s.metric(key)).collect();
        if values.iter().any(|v| !v.is_finite()) {
            return Err(CalibrateError::MixedManifest(format!(
                "non-finite metric {key}"
            )));
        }
        let baseline_ms = median(&values);
        let noise_nmad = normalized_mad(&values);
        if !noise_nmad.is_finite() {
            return Err(CalibrateError::Noisy {
                metric: key.into(),
                nmad: noise_nmad,
                limit: NMAD_LIMIT,
            });
        }
        if noise_nmad > NMAD_LIMIT {
            return Err(CalibrateError::Noisy {
                metric: key.into(),
                nmad: noise_nmad,
                limit: NMAD_LIMIT,
            });
        }
        if margin <= noise_nmad {
            return Err(CalibrateError::MarginBelowNoise {
                metric: key.into(),
                margin,
                noise_nmad,
            });
        }
        let noise_envelope_ms = baseline_ms * noise_nmad;
        let relative_limit_ms = baseline_ms * (1.0 + margin);
        built.push((
            key,
            MetricBaseline {
                baseline_ms,
                noise_nmad,
                noise_envelope_ms,
                margin,
                relative_limit_ms,
            },
        ));
    }

    let metrics = BaselineMetrics {
        median_p95_frame_service_ms: take_metric(&built, "median_p95_frame_service_ms"),
        median_p99_frame_service_ms: take_metric(&built, "median_p99_frame_service_ms"),
        median_sim_ms: take_metric(&built, "median_sim_ms"),
        median_upload_ms: take_metric(&built, "median_upload_ms"),
        median_gpu_queue_latency_ms: take_metric(&built, "median_gpu_queue_latency_ms"),
    };

    let candidate = Baseline {
        schema_version: BASELINE_SCHEMA.into(),
        enabled: false,
        review_required: true,
        platform: dataset.platform.clone(),
        backend: dataset.backend.clone(),
        manifest: dataset.manifest.clone(),
        provenance: dataset.provenance,
        sample_count: n,
        nmad_limit: NMAD_LIMIT,
        metrics,
        review: None,
    };
    candidate.validate_flags()?;
    Ok(candidate)
}

fn take_metric(built: &[(&str, MetricBaseline)], key: &str) -> MetricBaseline {
    built
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, m)| m.clone())
        .expect("metric present")
}

/// Explicit owner enable. Never silent.
pub fn enable_reviewed_baseline(
    candidate: &Baseline,
    review: ReviewRecord,
) -> Result<Baseline, CalibrateError> {
    if candidate.enabled {
        return Err(CalibrateError::AlreadyEnabled);
    }
    if candidate.provenance != Provenance::Native {
        return Err(CalibrateError::NonNativeProvenance(
            candidate.provenance.as_str().into(),
        ));
    }
    if review.reviewer.trim().is_empty() || review.evidence_ref.trim().is_empty() {
        return Err(CalibrateError::MissingReview);
    }
    // Re-check margin > noise on every metric (candidate may be hand-edited).
    for key in METRIC_KEYS {
        let m = candidate.metrics.get(key).expect("known metric");
        if m.margin <= m.noise_nmad {
            return Err(CalibrateError::MarginBelowNoise {
                metric: key.into(),
                margin: m.margin,
                noise_nmad: m.noise_nmad,
            });
        }
        if m.noise_nmad > NMAD_LIMIT {
            return Err(CalibrateError::Noisy {
                metric: key.into(),
                nmad: m.noise_nmad,
                limit: NMAD_LIMIT,
            });
        }
    }

    let mut enabled = candidate.clone();
    enabled.enabled = true;
    enabled.review_required = false;
    enabled.review = Some(review);
    enabled.validate_flags()?;
    Ok(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_manifest() -> CalibrationManifest {
        CalibrationManifest {
            scenario_version: "scenario-v1".into(),
            scenario_sha256: "abc123".into(),
            shader_manifest_version: 1,
            os_build: "Ubuntu 24.04".into(),
            driver: "amdgpu 23.2".into(),
            cpu: "8600G".into(),
            gpu: "RX 6400".into(),
            policy_id: "phase0-absolute-v1".into(),
        }
    }

    fn stable_sample(i: usize, scale: f64) -> CalibrationSample {
        // Tiny jitter keeps nmad well under 3%.
        let j = (i % 5) as f64 * 0.001 * scale;
        CalibrationSample {
            run_id: format!("run-{i:03}"),
            median_p95_frame_service_ms: 12.0 * scale + j,
            median_p99_frame_service_ms: 14.0 * scale + j,
            median_sim_ms: 2.0 * scale + j * 0.1,
            median_upload_ms: 1.0 * scale + j * 0.05,
            median_gpu_queue_latency_ms: 3.0 * scale + j * 0.2,
            manifest_override: None,
        }
    }

    fn stable_dataset(n: usize) -> CalibrationDataset {
        CalibrationDataset {
            schema_version: DATASET_SCHEMA.into(),
            platform: "linux-x86_64".into(),
            backend: "vulkan".into(),
            manifest: base_manifest(),
            provenance: Provenance::Native,
            samples: (0..n).map(|i| stable_sample(i, 1.0)).collect(),
        }
    }

    fn noisy_dataset(n: usize) -> CalibrationDataset {
        let mut ds = stable_dataset(n);
        // Alternate low/high so MAD/median >> 3%.
        for (i, s) in ds.samples.iter_mut().enumerate() {
            if i % 2 == 0 {
                s.median_p95_frame_service_ms = 10.0;
                s.median_p99_frame_service_ms = 12.0;
            } else {
                s.median_p95_frame_service_ms = 20.0;
                s.median_p99_frame_service_ms = 24.0;
            }
        }
        ds
    }

    #[test]
    fn requires_50_clean_reports() {
        let ds = stable_dataset(49);
        let err = derive_baseline_candidate(&ds, 0.05).unwrap_err();
        assert_eq!(err, CalibrateError::Underfilled(49));
    }

    #[test]
    fn mixed_manifest_rejects() {
        let mut ds = stable_dataset(50);
        let mut bad = base_manifest();
        bad.driver = "drifted-driver".into();
        ds.samples[10].manifest_override = Some(bad);
        let err = derive_baseline_candidate(&ds, 0.05).unwrap_err();
        match err {
            CalibrateError::MixedManifest(msg) => {
                assert!(msg.contains("run-010"), "{msg}");
            }
            other => panic!("expected MixedManifest, got {other:?}"),
        }
    }

    #[test]
    fn margin_must_exceed_noise() {
        let ds = stable_dataset(50);
        // Compute actual noise, then pick margin below it.
        let values: Vec<f64> = ds
            .samples
            .iter()
            .map(|s| s.median_p95_frame_service_ms)
            .collect();
        let noise = normalized_mad(&values);
        assert!(noise.is_finite() && noise > 0.0, "noise={noise}");
        let too_small = noise * 0.5;
        let err = derive_baseline_candidate(&ds, too_small).unwrap_err();
        match err {
            CalibrateError::MarginBelowNoise {
                metric,
                margin,
                noise_nmad,
            } => {
                assert_eq!(metric, "median_p95_frame_service_ms");
                assert!((margin - too_small).abs() < 1e-15);
                assert!((noise_nmad - noise).abs() < 1e-12);
            }
            other => panic!("expected MarginBelowNoise, got {other:?}"),
        }
    }

    #[test]
    fn output_requires_review_flag() {
        let ds = stable_dataset(50);
        let cand = derive_baseline_candidate(&ds, 0.05).unwrap();
        assert!(!cand.enabled, "never auto-enable");
        assert!(cand.review_required);
        assert!(cand.review.is_none());
        assert_eq!(cand.sample_count, 50);
        assert_eq!(cand.schema_version, BASELINE_SCHEMA);
        // Margin stored explicitly on every metric.
        assert!((cand.metrics.median_p95_frame_service_ms.margin - 0.05).abs() < 1e-15);
        assert!(
            cand.metrics.median_p95_frame_service_ms.relative_limit_ms
                > cand.metrics.median_p95_frame_service_ms.baseline_ms
        );
    }

    #[test]
    fn synthetic_stable_accepts() {
        let ds = stable_dataset(50);
        let cand = derive_baseline_candidate(&ds, 0.05).unwrap();
        assert!(cand.metrics.median_p95_frame_service_ms.noise_nmad < NMAD_LIMIT);
        assert!(cand.metrics.median_sim_ms.noise_nmad < NMAD_LIMIT);
    }

    #[test]
    fn synthetic_noisy_rejects() {
        let ds = noisy_dataset(50);
        let err = derive_baseline_candidate(&ds, 0.5).unwrap_err();
        match err {
            CalibrateError::Noisy {
                metric,
                nmad,
                limit,
            } => {
                assert_eq!(metric, "median_p95_frame_service_ms");
                assert!(nmad > limit);
            }
            other => panic!("expected Noisy, got {other:?}"),
        }
    }

    #[test]
    fn synthetic_drift_rejects() {
        let mut ds = stable_dataset(50);
        let mut drifted = base_manifest();
        drifted.scenario_sha256 = "ddddrift".into();
        ds.samples[0].manifest_override = Some(drifted);
        assert!(matches!(
            derive_baseline_candidate(&ds, 0.05),
            Err(CalibrateError::MixedManifest(_))
        ));
    }

    #[test]
    fn enable_reviewed_sets_flags() {
        let ds = stable_dataset(50);
        let cand = derive_baseline_candidate(&ds, 0.05).unwrap();
        let enabled = enable_reviewed_baseline(
            &cand,
            ReviewRecord {
                reviewer: "owner".into(),
                reviewed_at: "2026-08-02T00:00:00Z".into(),
                evidence_ref: "lab/calibration/ubuntu-run-set-1".into(),
                notes: "pilot ok".into(),
            },
        )
        .unwrap();
        assert!(enabled.enabled);
        assert!(!enabled.review_required);
        assert_eq!(enabled.review.as_ref().unwrap().reviewer, "owner");
    }

    #[test]
    fn enable_without_reviewer_fails() {
        let ds = stable_dataset(50);
        let cand = derive_baseline_candidate(&ds, 0.05).unwrap();
        let err = enable_reviewed_baseline(
            &cand,
            ReviewRecord {
                reviewer: "  ".into(),
                reviewed_at: "2026-08-02T00:00:00Z".into(),
                evidence_ref: "ev".into(),
                notes: String::new(),
            },
        )
        .unwrap_err();
        assert_eq!(err, CalibrateError::MissingReview);
    }

    #[test]
    fn synthetic_provenance_cannot_enable() {
        let mut ds = stable_dataset(50);
        ds.provenance = Provenance::Synthetic;
        let cand = derive_baseline_candidate(&ds, 0.05).unwrap();
        assert_eq!(cand.provenance, Provenance::Synthetic);
        let err = enable_reviewed_baseline(
            &cand,
            ReviewRecord {
                reviewer: "owner".into(),
                reviewed_at: "2026-08-05T00:00:00Z".into(),
                evidence_ref: "lab/calibration/synthetic-set".into(),
                notes: "must refuse".into(),
            },
        )
        .unwrap_err();
        assert_eq!(err, CalibrateError::NonNativeProvenance("synthetic".into()));
    }

    #[test]
    fn unspecified_provenance_cannot_enable() {
        let mut ds = stable_dataset(50);
        ds.provenance = Provenance::Unspecified;
        let cand = derive_baseline_candidate(&ds, 0.05).unwrap();
        let err = enable_reviewed_baseline(
            &cand,
            ReviewRecord {
                reviewer: "owner".into(),
                reviewed_at: "2026-08-05T00:00:00Z".into(),
                evidence_ref: "ev".into(),
                notes: String::new(),
            },
        )
        .unwrap_err();
        assert_eq!(
            err,
            CalibrateError::NonNativeProvenance("unspecified".into())
        );
    }

    #[test]
    fn fifty_clean_ok() {
        let ds = stable_dataset(50);
        assert!(derive_baseline_candidate(&ds, 0.05).is_ok());
    }
}
