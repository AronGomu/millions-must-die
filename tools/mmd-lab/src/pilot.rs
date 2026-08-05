//! T26 pilot dataset pipeline.
//!
//! Assembles a 150-report (50/lane) pilot dataset from lane reports, validates
//! acceptance (exact counts, frozen manifests, noise envelope), and emits
//! DISABLED relative-baseline candidates plus a not-reviewed golden tolerance
//! record bounded by observed deltas only.
//!
//! Hardware deferral policy (2026-08-05): no physical lab exists, so the 150
//! real lane runs are deferred-hw. This module proves the pipeline end-to-end
//! on deterministic synthetic data; synthetic provenance can never enable.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use mmd_engine::render::GOLDEN_MAX_CHANNEL_DELTA_POLICY;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::calibrate::{
    Baseline, CalibrateError, CalibrationDataset, CalibrationManifest, CalibrationSample,
    DATASET_SCHEMA, MIN_CLEAN_REPORTS, Provenance, derive_baseline_candidate,
};

pub const PILOT_REPORT_SCHEMA: &str = "pilot-report-v1";
pub const GOLDEN_TOLERANCE_SCHEMA: &str = "golden-tolerance-review-v1";

/// Exact total pilot report count: 50 clean reports per lane, 3 lanes.
pub const PILOT_TOTAL_REPORTS: usize = MIN_CLEAN_REPORTS * PILOT_LANES.len();

/// Frozen pilot lane spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PilotLane {
    pub lane: &'static str,
    pub platform: &'static str,
    /// Lab backend id (matches lane evidence/gate naming).
    pub backend: &'static str,
    /// Golden family directory under `lab/goldens/`.
    pub golden_family: &'static str,
    /// SDL backend id used by golden manifests.
    pub golden_backend: &'static str,
    /// Tracked candidate file stem under `lab/baselines/`.
    pub baseline_file: &'static str,
}

pub const PILOT_LANES: [PilotLane; 3] = [
    PilotLane {
        lane: "ubuntu",
        platform: "linux-x86_64",
        backend: "vulkan",
        golden_family: "linux-vulkan",
        golden_backend: "vulkan",
        baseline_file: "ubuntu-vulkan.json",
    },
    PilotLane {
        lane: "windows",
        platform: "windows-x86_64",
        backend: "d3d12",
        golden_family: "windows-d3d12",
        golden_backend: "direct3d12",
        baseline_file: "windows-d3d12.json",
    },
    PilotLane {
        lane: "macos",
        platform: "macos-arm64",
        backend: "metal",
        golden_family: "macos-metal",
        golden_backend: "metal",
        baseline_file: "macos-metal.json",
    },
];

#[derive(Debug, Error, PartialEq)]
pub enum PilotError {
    #[error("pilot requires exactly {PILOT_TOTAL_REPORTS} reports (50/lane); got {0}")]
    WrongTotal(usize),
    #[error("pilot report schema_version must be {PILOT_REPORT_SCHEMA}, got {0}")]
    BadSchema(String),
    #[error("unknown pilot lane '{lane}' (platform={platform}, backend={backend})")]
    UnknownLane {
        lane: String,
        platform: String,
        backend: String,
    },
    #[error("missing pilot lane {0}")]
    MissingLane(String),
    #[error("lane {lane} requires exactly {MIN_CLEAN_REPORTS} clean reports; got {got}")]
    LaneCount { lane: String, got: usize },
    #[error("lane {lane} manifest drift at report {run_id}; pilot manifests must be frozen")]
    ManifestDrift { lane: String, run_id: String },
    #[error("lane {lane} mixes data provenance; pilot lanes must be uniform")]
    MixedProvenance { lane: String },
    #[error("lane {lane}: {source}")]
    Lane {
        lane: String,
        source: CalibrateError,
    },
    #[error(
        "golden tolerance overreach: requested {requested} exceeds observed max delta {observed}"
    )]
    ToleranceOverreach { requested: u8, observed: u8 },
    #[error(
        "golden tolerance {tolerance} exceeds policy cap {GOLDEN_MAX_CHANNEL_DELTA_POLICY}; \
         widening requires reviewed native evidence"
    )]
    TolerancePolicyCap { tolerance: u8 },
    #[error("io: {0}")]
    Io(String),
    #[error("json: {0}")]
    Json(String),
}

/// One lane report feeding the pilot dataset (from a full gate run summary or
/// the synthetic generator).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PilotReport {
    pub schema_version: String,
    pub lane: String,
    pub platform: String,
    pub backend: String,
    pub provenance: Provenance,
    pub manifest: CalibrationManifest,
    pub sample: CalibrationSample,
}

impl PilotReport {
    pub fn from_json(s: &str) -> Result<Self, PilotError> {
        let r: Self = serde_json::from_str(s).map_err(|e| PilotError::Json(e.to_string()))?;
        if r.schema_version != PILOT_REPORT_SCHEMA {
            return Err(PilotError::BadSchema(r.schema_version));
        }
        Ok(r)
    }
}

/// Disabled baseline candidate bound to its lane/output naming.
#[derive(Debug, Clone, PartialEq)]
pub struct PilotCandidate {
    pub lane: &'static str,
    pub baseline_file: &'static str,
    pub golden_family: &'static str,
    pub baseline: Baseline,
}

/// One golden family tolerance derivation, bounded by observed deltas only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoldenToleranceEntry {
    pub family: String,
    pub backend: String,
    pub observed_max_channel_delta: u8,
    /// Derived tolerance; never exceeds the observed max delta.
    pub tolerance: u8,
    pub reviewed: bool,
    pub provenance: Provenance,
    pub note: String,
}

/// Tracked golden tolerance review index (`lab/goldens/manifest.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoldenToleranceReview {
    pub schema_version: String,
    pub provenance: Provenance,
    /// False until an owner reviews real native image deltas.
    pub reviewed: bool,
    pub entries: Vec<GoldenToleranceEntry>,
}

impl GoldenToleranceReview {
    pub fn from_json(s: &str) -> Result<Self, PilotError> {
        let r: Self = serde_json::from_str(s).map_err(|e| PilotError::Json(e.to_string()))?;
        if r.schema_version != GOLDEN_TOLERANCE_SCHEMA {
            return Err(PilotError::BadSchema(r.schema_version));
        }
        Ok(r)
    }
}

fn lane_spec(lane: &str) -> Option<&'static PilotLane> {
    PILOT_LANES.iter().find(|s| s.lane == lane)
}

/// Validate the 150-report pilot dataset and split it into per-lane
/// calibration datasets. Rejects wrong totals, unknown lanes, per-lane count
/// mismatches, manifest drift, and mixed provenance.
pub fn assemble_pilot_datasets(
    reports: &[PilotReport],
) -> Result<Vec<CalibrationDataset>, PilotError> {
    if reports.len() != PILOT_TOTAL_REPORTS {
        return Err(PilotError::WrongTotal(reports.len()));
    }

    let mut by_lane: BTreeMap<&str, Vec<&PilotReport>> = BTreeMap::new();
    for r in reports {
        if r.schema_version != PILOT_REPORT_SCHEMA {
            return Err(PilotError::BadSchema(r.schema_version.clone()));
        }
        let spec = lane_spec(&r.lane).ok_or_else(|| PilotError::UnknownLane {
            lane: r.lane.clone(),
            platform: r.platform.clone(),
            backend: r.backend.clone(),
        })?;
        if r.platform != spec.platform || r.backend != spec.backend {
            return Err(PilotError::UnknownLane {
                lane: r.lane.clone(),
                platform: r.platform.clone(),
                backend: r.backend.clone(),
            });
        }
        by_lane.entry(spec.lane).or_default().push(r);
    }

    let mut datasets = Vec::with_capacity(PILOT_LANES.len());
    for spec in &PILOT_LANES {
        let lane_reports = by_lane
            .get(spec.lane)
            .ok_or_else(|| PilotError::MissingLane(spec.lane.into()))?;
        if lane_reports.len() != MIN_CLEAN_REPORTS {
            return Err(PilotError::LaneCount {
                lane: spec.lane.into(),
                got: lane_reports.len(),
            });
        }
        let manifest = lane_reports[0].manifest.clone();
        let expected_fp = manifest.fingerprint();
        let provenance = lane_reports[0].provenance;
        for r in lane_reports {
            if r.manifest.fingerprint() != expected_fp {
                return Err(PilotError::ManifestDrift {
                    lane: spec.lane.into(),
                    run_id: r.sample.run_id.clone(),
                });
            }
            if r.provenance != provenance {
                return Err(PilotError::MixedProvenance {
                    lane: spec.lane.into(),
                });
            }
        }
        datasets.push(CalibrationDataset {
            schema_version: DATASET_SCHEMA.into(),
            platform: spec.platform.into(),
            backend: spec.backend.into(),
            manifest,
            provenance,
            samples: lane_reports.iter().map(|r| r.sample.clone()).collect(),
        });
    }
    Ok(datasets)
}

/// Assemble the pilot dataset and derive one DISABLED baseline candidate per
/// lane. Noisy sets and margin-below-noise reject via the calibration engine.
pub fn derive_pilot_candidates(
    reports: &[PilotReport],
    margin: f64,
) -> Result<Vec<PilotCandidate>, PilotError> {
    let datasets = assemble_pilot_datasets(reports)?;
    let mut candidates = Vec::with_capacity(datasets.len());
    for (spec, dataset) in PILOT_LANES.iter().zip(&datasets) {
        let baseline =
            derive_baseline_candidate(dataset, margin).map_err(|source| PilotError::Lane {
                lane: spec.lane.into(),
                source,
            })?;
        candidates.push(PilotCandidate {
            lane: spec.lane,
            baseline_file: spec.baseline_file,
            golden_family: spec.golden_family,
            baseline,
        });
    }
    Ok(candidates)
}

/// Derive a golden tolerance entry bounded by observed deltas only.
/// Synthetic pipeline runs record a zero/synthetic delta and stay not-reviewed.
pub fn derive_golden_tolerance_entry(
    spec: &PilotLane,
    observed_deltas: &[u8],
    provenance: Provenance,
) -> GoldenToleranceEntry {
    let observed = observed_deltas.iter().copied().max().unwrap_or(0);
    let note = match provenance {
        Provenance::Native => "derived from reviewed native image deltas".into(),
        _ => format!(
            "{} pipeline proof; zero/synthetic delta recorded; native capture deferred-hw; \
             not reviewed",
            provenance.as_str()
        ),
    };
    GoldenToleranceEntry {
        family: spec.golden_family.into(),
        backend: spec.golden_backend.into(),
        observed_max_channel_delta: observed,
        tolerance: observed,
        reviewed: false,
        provenance,
        note,
    }
}

/// Refuse tolerance requests wider than the observed delta or the exact-match
/// policy cap.
pub fn check_tolerance_request(
    requested: u8,
    entry: &GoldenToleranceEntry,
) -> Result<(), PilotError> {
    if requested > entry.observed_max_channel_delta {
        return Err(PilotError::ToleranceOverreach {
            requested,
            observed: entry.observed_max_channel_delta,
        });
    }
    if requested > GOLDEN_MAX_CHANNEL_DELTA_POLICY {
        return Err(PilotError::TolerancePolicyCap {
            tolerance: requested,
        });
    }
    Ok(())
}

/// Build the tracked golden tolerance review index for the pilot candidates.
pub fn build_golden_tolerance_review(
    candidates: &[PilotCandidate],
    observed_deltas: &[u8],
) -> GoldenToleranceReview {
    let entries: Vec<GoldenToleranceEntry> = candidates
        .iter()
        .map(|c| {
            let spec = lane_spec(c.lane).expect("known lane");
            derive_golden_tolerance_entry(spec, observed_deltas, c.baseline.provenance)
        })
        .collect();
    let provenance = candidates
        .first()
        .map(|c| c.baseline.provenance)
        .unwrap_or_default();
    GoldenToleranceReview {
        schema_version: GOLDEN_TOLERANCE_SCHEMA.into(),
        provenance,
        reviewed: false,
        entries,
    }
}

/// Build, bound-check, write, and re-load the tracked golden tolerance review
/// index. Refuses tolerance overreach before anything reaches disk.
pub fn write_golden_tolerance_review(
    path: &Path,
    candidates: &[PilotCandidate],
    observed_deltas: &[u8],
) -> Result<GoldenToleranceReview, PilotError> {
    let review = build_golden_tolerance_review(candidates, observed_deltas);
    for entry in &review.entries {
        check_tolerance_request(entry.tolerance, entry)?;
    }
    let body =
        serde_json::to_string_pretty(&review).map_err(|e| PilotError::Json(e.to_string()))?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|e| PilotError::Io(e.to_string()))?;
    }
    fs::write(path, &body).map_err(|e| PilotError::Io(e.to_string()))?;
    // Integrity: the written body must parse back as a valid review index.
    GoldenToleranceReview::from_json(&body)
}

// ---------------------------------------------------------------------------
// Deterministic synthetic generator (pipeline proof only; never enableable).
// ---------------------------------------------------------------------------

/// Knuth LCG; deterministic across platforms.
fn lcg_next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state
}

/// Uniform jitter in `[-1, 1)`.
fn jitter(state: &mut u64) -> f64 {
    ((lcg_next(state) >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
}

fn synth_manifest(spec: &PilotLane) -> CalibrationManifest {
    CalibrationManifest {
        scenario_version: "technical-prototype-v1".into(),
        scenario_sha256: format!("synthetic-pilot-{}", spec.lane),
        shader_manifest_version: 1,
        os_build: format!("{} (synthetic pilot)", spec.lane),
        driver: "synthetic-driver".into(),
        cpu: "synthetic-cpu".into(),
        gpu: "synthetic-gpu".into(),
        policy_id: "phase0-absolute-v1".into(),
    }
}

/// Generate the full deterministic 150-report synthetic pilot set.
/// Relative jitter stays ~0.2% so the noise envelope passes; provenance is
/// `synthetic` on every report, which structurally blocks enable.
pub fn synth_pilot_reports(seed: u64) -> Vec<PilotReport> {
    // (p95, p99, sim, upload, gpu_queue) baseline ms per lane.
    const BASES: [(f64, f64, f64, f64, f64); 3] = [
        (12.0, 14.0, 2.0, 1.0, 3.0),
        (13.0, 15.5, 2.2, 1.2, 3.4),
        (11.0, 13.0, 1.8, 0.9, 2.8),
    ];
    const REL_JITTER: f64 = 0.002;

    // Quantize to 1e-6 ms so JSON round-trips are bit-exact.
    fn quant(v: f64) -> f64 {
        (v * 1e6).round() / 1e6
    }

    let mut state = seed ^ 0x9e37_79b9_7f4a_7c15;
    let mut reports = Vec::with_capacity(PILOT_TOTAL_REPORTS);
    for (spec, base) in PILOT_LANES.iter().zip(BASES) {
        let manifest = synth_manifest(spec);
        for i in 0..MIN_CLEAN_REPORTS {
            let mut m = |b: f64| quant(b * (1.0 + REL_JITTER * jitter(&mut state)));
            let sample = CalibrationSample {
                run_id: format!("synthetic-{}-{i:03}", spec.lane),
                median_p95_frame_service_ms: m(base.0),
                median_p99_frame_service_ms: m(base.1),
                median_sim_ms: m(base.2),
                median_upload_ms: m(base.3),
                median_gpu_queue_latency_ms: m(base.4),
                manifest_override: None,
            };
            reports.push(PilotReport {
                schema_version: PILOT_REPORT_SCHEMA.into(),
                lane: spec.lane.into(),
                platform: spec.platform.into(),
                backend: spec.backend.into(),
                provenance: Provenance::Synthetic,
                manifest: manifest.clone(),
                sample,
            });
        }
    }
    reports
}

// ---------------------------------------------------------------------------
// Bulk report IO (bulk data stays external/untracked; generator is tracked).
// ---------------------------------------------------------------------------

/// Write reports as `<dir>/<lane>/report-<idx>.json`.
pub fn write_pilot_reports(dir: &Path, reports: &[PilotReport]) -> Result<(), PilotError> {
    let mut per_lane: BTreeMap<&str, usize> = BTreeMap::new();
    for r in reports {
        let idx = per_lane.entry(r.lane.as_str()).or_insert(0);
        let lane_dir = dir.join(&r.lane);
        fs::create_dir_all(&lane_dir).map_err(|e| PilotError::Io(e.to_string()))?;
        let path = lane_dir.join(format!("report-{:03}.json", *idx));
        let body = serde_json::to_string_pretty(r).map_err(|e| PilotError::Json(e.to_string()))?;
        fs::write(&path, body).map_err(|e| PilotError::Io(e.to_string()))?;
        *idx += 1;
    }
    Ok(())
}

/// Load every `*.json` report under `<dir>/<lane>/` in deterministic order.
pub fn load_pilot_reports(dir: &Path) -> Result<Vec<PilotReport>, PilotError> {
    let mut files: Vec<PathBuf> = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| PilotError::Io(e.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|e| PilotError::Io(e.to_string()))?;
        let path = entry.path();
        if path.is_dir() {
            let inner = fs::read_dir(&path).map_err(|e| PilotError::Io(e.to_string()))?;
            for f in inner {
                let f = f.map_err(|e| PilotError::Io(e.to_string()))?;
                let fp = f.path();
                if fp.extension().is_some_and(|e| e == "json") {
                    files.push(fp);
                }
            }
        } else if path.extension().is_some_and(|e| e == "json") {
            files.push(path);
        }
    }
    files.sort();
    let mut reports = Vec::with_capacity(files.len());
    for path in files {
        let body = fs::read_to_string(&path).map_err(|e| PilotError::Io(e.to_string()))?;
        reports.push(PilotReport::from_json(&body)?);
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibrate::{ReviewRecord, enable_reviewed_baseline};

    fn review() -> ReviewRecord {
        ReviewRecord {
            reviewer: "owner".into(),
            reviewed_at: "2026-08-05T00:00:00Z".into(),
            evidence_ref: "lab/calibration/pilot-set-1".into(),
            notes: "test".into(),
        }
    }

    #[test]
    fn synth_is_deterministic() {
        assert_eq!(synth_pilot_reports(42), synth_pilot_reports(42));
        assert_ne!(synth_pilot_reports(42), synth_pilot_reports(43));
        assert_eq!(synth_pilot_reports(42).len(), PILOT_TOTAL_REPORTS);
    }

    #[test]
    fn pilot_requires_150_reports() {
        let mut reports = synth_pilot_reports(42);
        reports.pop();
        let err = assemble_pilot_datasets(&reports).unwrap_err();
        assert_eq!(err, PilotError::WrongTotal(149));
    }

    #[test]
    fn pilot_accepts_exact_150() {
        let reports = synth_pilot_reports(42);
        let datasets = assemble_pilot_datasets(&reports).unwrap();
        assert_eq!(datasets.len(), 3);
        for ds in &datasets {
            assert_eq!(ds.samples.len(), MIN_CLEAN_REPORTS);
            assert_eq!(ds.provenance, Provenance::Synthetic);
        }
    }

    #[test]
    fn pilot_manifest_drift_rejects() {
        let mut reports = synth_pilot_reports(42);
        reports[10].manifest.driver = "drifted-driver".into();
        let err = assemble_pilot_datasets(&reports).unwrap_err();
        match err {
            PilotError::ManifestDrift { lane, run_id } => {
                assert_eq!(lane, "ubuntu");
                assert_eq!(run_id, reports[10].sample.run_id);
            }
            other => panic!("expected ManifestDrift, got {other:?}"),
        }
    }

    #[test]
    fn pilot_lane_count_must_be_exact() {
        // Keep total at 150 but shift one report between lanes.
        let mut reports = synth_pilot_reports(42);
        let macos_idx = reports
            .iter()
            .position(|r| r.lane == "macos")
            .expect("macos report");
        reports.remove(macos_idx);
        let extra = reports[0].clone();
        reports.push(extra);
        let err = assemble_pilot_datasets(&reports).unwrap_err();
        assert_eq!(
            err,
            PilotError::LaneCount {
                lane: "ubuntu".into(),
                got: 51
            }
        );
    }

    #[test]
    fn pilot_noisy_set_rejects() {
        let mut reports = synth_pilot_reports(42);
        for (i, r) in reports
            .iter_mut()
            .filter(|r| r.lane == "ubuntu")
            .enumerate()
        {
            r.sample.median_p95_frame_service_ms = if i % 2 == 0 { 10.0 } else { 20.0 };
        }
        let err = derive_pilot_candidates(&reports, 0.5).unwrap_err();
        match err {
            PilotError::Lane { lane, source } => {
                assert_eq!(lane, "ubuntu");
                assert!(matches!(source, CalibrateError::Noisy { .. }), "{source}");
            }
            other => panic!("expected noisy lane, got {other:?}"),
        }
    }

    #[test]
    fn pilot_candidates_are_disabled_synthetic() {
        let reports = synth_pilot_reports(42);
        let candidates = derive_pilot_candidates(&reports, 0.05).unwrap();
        assert_eq!(candidates.len(), 3);
        for c in &candidates {
            assert!(!c.baseline.enabled);
            assert!(c.baseline.review_required);
            assert!(c.baseline.review.is_none());
            assert_eq!(c.baseline.provenance, Provenance::Synthetic);
            c.baseline.validate_flags().unwrap();
        }
    }

    #[test]
    fn synthetic_provenance_cannot_enable() {
        let reports = synth_pilot_reports(42);
        let candidates = derive_pilot_candidates(&reports, 0.05).unwrap();
        for c in &candidates {
            let err = enable_reviewed_baseline(&c.baseline, review()).unwrap_err();
            assert_eq!(
                err,
                CalibrateError::NonNativeProvenance("synthetic".into()),
                "lane {}",
                c.lane
            );
        }
    }

    #[test]
    fn enabled_baseline_has_review_record() {
        // Native-provenance pilot data (the only kind that may enable) must
        // carry a full owner review record after enable.
        let mut reports = synth_pilot_reports(42);
        for r in &mut reports {
            r.provenance = Provenance::Native;
        }
        let candidates = derive_pilot_candidates(&reports, 0.05).unwrap();
        let enabled = enable_reviewed_baseline(&candidates[0].baseline, review()).unwrap();
        let rec = enabled.review.as_ref().expect("review record");
        assert_eq!(rec.reviewer, "owner");
        assert_eq!(rec.reviewed_at, "2026-08-05T00:00:00Z");
        assert_eq!(rec.evidence_ref, "lab/calibration/pilot-set-1");

        // An enabled baseline without a review record is structurally invalid.
        let mut forged = candidates[0].baseline.clone();
        forged.enabled = true;
        forged.review_required = false;
        assert_eq!(
            forged.validate_flags().unwrap_err(),
            CalibrateError::MissingReview
        );
    }

    #[test]
    fn golden_tolerance_covers_only_observed_delta() {
        let spec = &PILOT_LANES[0];
        let entry = derive_golden_tolerance_entry(spec, &[0, 0, 0], Provenance::Synthetic);
        assert_eq!(entry.observed_max_channel_delta, 0);
        assert_eq!(entry.tolerance, 0, "tolerance bounded by observed delta");
        assert!(!entry.reviewed);

        // Requesting beyond the observed delta is overreach.
        assert_eq!(
            check_tolerance_request(1, &entry).unwrap_err(),
            PilotError::ToleranceOverreach {
                requested: 1,
                observed: 0
            }
        );
        assert!(check_tolerance_request(0, &entry).is_ok());

        // Even an observed nonzero delta cannot pass the exact-match policy cap.
        let wide = derive_golden_tolerance_entry(spec, &[2, 1], Provenance::Synthetic);
        assert_eq!(wide.tolerance, 2);
        assert_eq!(
            check_tolerance_request(2, &wide).unwrap_err(),
            PilotError::TolerancePolicyCap { tolerance: 2 }
        );
    }

    #[test]
    fn report_io_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let reports = synth_pilot_reports(42);
        write_pilot_reports(dir.path(), &reports).unwrap();
        let loaded = load_pilot_reports(dir.path()).unwrap();
        assert_eq!(loaded.len(), PILOT_TOTAL_REPORTS);
        // Order differs (sorted by path) but the set assembles identically.
        let datasets = assemble_pilot_datasets(&loaded).unwrap();
        assert_eq!(datasets, assemble_pilot_datasets(&reports).unwrap());
    }

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn tracked_baseline_candidates_stay_disabled() {
        // The repo-tracked pilot candidates must remain structurally disabled,
        // review-pending, and synthetic so no gate can ever consume them.
        for spec in &PILOT_LANES {
            let path = workspace_root()
                .join("lab/baselines")
                .join(spec.baseline_file);
            let baseline = Baseline::load(&path)
                .unwrap_or_else(|e| panic!("tracked candidate {} unreadable: {e}", path.display()));
            assert!(!baseline.enabled, "{}", spec.baseline_file);
            assert!(baseline.review_required, "{}", spec.baseline_file);
            assert!(baseline.review.is_none(), "{}", spec.baseline_file);
            assert_eq!(
                baseline.provenance,
                Provenance::Synthetic,
                "{}",
                spec.baseline_file
            );
            baseline.validate_flags().unwrap();
            assert_eq!(
                enable_reviewed_baseline(&baseline, review()).unwrap_err(),
                CalibrateError::NonNativeProvenance("synthetic".into()),
                "{}",
                spec.baseline_file
            );
        }
    }

    #[test]
    fn tracked_golden_tolerance_review_stays_unreviewed() {
        let path = workspace_root().join("lab/goldens/manifest.json");
        let body = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("tracked review {} unreadable: {e}", path.display()));
        let review = GoldenToleranceReview::from_json(&body).unwrap();
        assert!(!review.reviewed);
        assert_eq!(review.provenance, Provenance::Synthetic);
        assert_eq!(review.entries.len(), PILOT_LANES.len());
        for (entry, spec) in review.entries.iter().zip(&PILOT_LANES) {
            assert_eq!(entry.family, spec.golden_family);
            assert_eq!(entry.backend, spec.golden_backend);
            assert!(!entry.reviewed);
            assert_eq!(entry.provenance, Provenance::Synthetic);
            assert!(
                entry.tolerance <= entry.observed_max_channel_delta,
                "tolerance bounded by observed delta"
            );
            assert!(check_tolerance_request(entry.tolerance, entry).is_ok());
        }
    }
}
