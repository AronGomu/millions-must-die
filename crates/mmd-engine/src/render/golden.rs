//! Backend-bound golden image correctness (T13).
//!
//! One golden family per backend (`lab/goldens/{linux-vulkan,windows-d3d12,macos-metal}`).
//! The comparator is renderer-specific on purpose: it validates one scene
//! (`static-demo-v1`) at the fixed gate resolution against one backend-bound
//! manifest. It is not a generic image-diff framework.
//!
//! Policy: exact compare (`max_channel_delta == 0`). A bounded channel or
//! perceptual tolerance may only be introduced after reviewed native evidence;
//! manifests requesting a looser tolerance are rejected outright so a candidate
//! cannot loosen the gate by editing its own manifest.
//!
//! Candidate output remains untrusted evidence: the coordinator later re-checks
//! manifest binding and diff policy from raw artifacts.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::backend::{BACKEND_D3D12, BACKEND_METAL, BACKEND_VULKAN};
use super::renderer::Readback;

/// Golden manifest schema identifier.
pub const GOLDEN_SCHEMA_VERSION: &str = "golden-manifest-v1";
/// Manifest status: image captured on real native hardware.
pub const GOLDEN_STATUS_CAPTURED: &str = "captured";
/// Manifest status: native capture deferred (no hardware); never comparable.
pub const GOLDEN_STATUS_PLACEHOLDER: &str = "placeholder-deferred-hw";
/// Exact-compare policy cap. Raising this requires reviewed native evidence
/// (see docs/lab/gpu-profiling.md) — not just a manifest edit.
pub const GOLDEN_MAX_CHANNEL_DELTA_POLICY: u8 = 0;
/// Scene identity every T13 golden binds to (`SpriteRenderer::static_demo_groups`).
pub const GOLDEN_SCENE_STATIC_DEMO: &str = "static-demo-v1";

/// Candidate frame written into a diff directory by [`write_golden_diff`].
pub const GOLDEN_DIFF_ACTUAL_PNG: &str = "actual.png";
/// Per-pixel drift mask written into a diff directory by [`write_golden_diff`].
pub const GOLDEN_DIFF_MASK_PNG: &str = "diff.png";
/// Machine-readable drift summary written by [`write_golden_diff`].
pub const GOLDEN_DIFF_SUMMARY_JSON: &str = "diff.json";
/// Mask colour marking a pixel that exceeded tolerance (opaque magenta).
const DIFF_MARK: [u8; 4] = [255, 0, 255, 255];
/// Mask colour marking a pixel within tolerance (opaque black).
const DIFF_KEEP: [u8; 4] = [0, 0, 0, 255];

/// Golden comparison errors (renderer-specific; deliberately not `RenderError`).
#[derive(Debug, thiserror::Error)]
pub enum GoldenError {
    #[error("golden io {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("golden manifest json {path}: {source}")]
    ManifestJson {
        path: String,
        source: serde_json::Error,
    },
    #[error("golden manifest {path}: missing field {field}")]
    ManifestField { path: String, field: &'static str },
    #[error("golden manifest schema {got:?} unsupported (want {GOLDEN_SCHEMA_VERSION:?})")]
    UnsupportedSchema { got: String },
    #[error("golden manifest backend {got:?} unknown")]
    UnknownBackend { got: String },
    #[error("golden manifest status {got:?} unknown")]
    UnknownStatus { got: String },
    #[error(
        "golden tolerance {requested} exceeds exact-compare policy cap \
         {GOLDEN_MAX_CHANNEL_DELTA_POLICY}; widening requires reviewed native evidence"
    )]
    ToleranceAbovePolicy { requested: u8 },
    #[error("golden is backend-bound: manifest backend {golden:?} cannot validate {host:?} output")]
    BackendMismatch { golden: String, host: String },
    #[error(
        "golden for {backend:?} is a deferred-hw placeholder; native capture required before compare"
    )]
    PlaceholderGolden { backend: String },
    #[error(
        "golden manifest drift on {field}: golden {golden:?} vs host {host:?}; \
         recalibration (reviewed re-capture) required before compare"
    )]
    ManifestDrift {
        field: &'static str,
        golden: String,
        host: String,
    },
    #[error(
        "golden dimensions {golden_width}x{golden_height} != candidate {got_width}x{got_height}"
    )]
    DimensionMismatch {
        golden_width: u32,
        golden_height: u32,
        got_width: u32,
        got_height: u32,
    },
    #[error("golden image byte length {got} != expected {expected}")]
    ImageSizeMismatch { expected: usize, got: usize },
    #[error("golden image hash mismatch for {file}: manifest {expected} actual {actual}")]
    ImageHashMismatch {
        file: String,
        expected: String,
        actual: String,
    },
    #[error("golden png {path}: {message}")]
    Png { path: String, message: String },
    #[error(
        "channel delta {delta} above tolerance {max} at ({x},{y}) channel {channel}: \
         golden {golden} candidate {candidate}"
    )]
    DeltaAboveTolerance {
        x: u32,
        y: u32,
        channel: usize,
        golden: u8,
        candidate: u8,
        delta: u8,
        max: u8,
    },
    #[error(
        "golden drift: {differing_pixels} of {total_pixels} pixels exceed tolerance {tolerance} \
         (max channel delta {max_channel_delta}); diff artifact written to {artifact_dir}"
    )]
    GoldenDrift {
        differing_pixels: u64,
        total_pixels: u64,
        max_channel_delta: u8,
        tolerance: u8,
        artifact_dir: String,
    },
    #[error(
        "benchmark report not bound to golden manifest on {field}: \
         golden {golden:?} vs report {report:?}"
    )]
    ReportBindingMismatch {
        field: &'static str,
        golden: String,
        report: String,
    },
}

/// Per-backend golden manifest (tracked next to the golden image).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldenManifest {
    pub schema_version: String,
    /// SDL GPU driver name this golden is bound to (`vulkan`/`direct3d12`/`metal`).
    pub backend: String,
    /// OS family the golden was (or must be) captured on.
    pub os: String,
    /// `captured` or `placeholder-deferred-hw`.
    pub status: String,
    /// Scene identity rendered into the golden.
    pub scene: String,
    pub width: u32,
    pub height: u32,
    /// Golden image file name relative to the manifest directory.
    pub image_file: String,
    /// SHA-256 of the golden PNG bytes; empty for placeholders.
    pub image_sha256: String,
    /// Adapter name reported by SDL at capture time.
    pub adapter: String,
    /// Free-form driver/capture provenance (recorded, drift-checked via adapter/hashes).
    pub driver_info: String,
    /// SHA-256 of `assets/sprites/generated/manifest.json` bytes (same
    /// definition as benchmark `ReportManifests.atlas_manifest_sha256`).
    pub atlas_manifest_sha256: String,
    /// `canonical_sha256` from `shaders/generated/manifest.json` (HLSL source pin).
    pub shader_canonical_sha256: String,
    /// Max allowed per-channel delta; policy-capped at 0 (exact).
    pub max_channel_delta: u8,
}

impl GoldenManifest {
    /// Structural + policy validation (schema, backend, status, tolerance cap).
    pub fn validate(&self) -> Result<(), GoldenError> {
        if self.schema_version != GOLDEN_SCHEMA_VERSION {
            return Err(GoldenError::UnsupportedSchema {
                got: self.schema_version.clone(),
            });
        }
        golden_family_dir(&self.backend)?;
        if self.status != GOLDEN_STATUS_CAPTURED && self.status != GOLDEN_STATUS_PLACEHOLDER {
            return Err(GoldenError::UnknownStatus {
                got: self.status.clone(),
            });
        }
        if self.max_channel_delta > GOLDEN_MAX_CHANNEL_DELTA_POLICY {
            return Err(GoldenError::ToleranceAbovePolicy {
                requested: self.max_channel_delta,
            });
        }
        Ok(())
    }
}

/// Current-host identity the golden must bind to before pixels are compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostBinding {
    pub backend: String,
    pub os: String,
    pub adapter: String,
    pub atlas_manifest_sha256: String,
    pub shader_canonical_sha256: String,
}

/// Golden family directory name for a backend (`lab/goldens/<family>`).
pub fn golden_family_dir(backend: &str) -> Result<&'static str, GoldenError> {
    match backend {
        BACKEND_VULKAN => Ok("linux-vulkan"),
        BACKEND_D3D12 => Ok("windows-d3d12"),
        BACKEND_METAL => Ok("macos-metal"),
        other => Err(GoldenError::UnknownBackend { got: other.into() }),
    }
}

/// Load + validate a golden manifest from `<family>/manifest.json`.
pub fn load_golden_manifest(path: &Path) -> Result<GoldenManifest, GoldenError> {
    let bytes = std::fs::read(path).map_err(|source| GoldenError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let manifest: GoldenManifest =
        serde_json::from_slice(&bytes).map_err(|source| GoldenError::ManifestJson {
            path: path.display().to_string(),
            source,
        })?;
    manifest.validate()?;
    Ok(manifest)
}

/// Load the golden image: verify PNG bytes hash against the manifest, then
/// decode to tightly-packed RGBA8 of exactly `width * height * 4` bytes.
pub fn load_golden_image(dir: &Path, manifest: &GoldenManifest) -> Result<Vec<u8>, GoldenError> {
    manifest.validate()?;
    if manifest.status != GOLDEN_STATUS_CAPTURED {
        return Err(GoldenError::PlaceholderGolden {
            backend: manifest.backend.clone(),
        });
    }
    let path = dir.join(&manifest.image_file);
    let bytes = std::fs::read(&path).map_err(|source| GoldenError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let actual = sha256_hex(&bytes);
    if actual != manifest.image_sha256 {
        return Err(GoldenError::ImageHashMismatch {
            file: manifest.image_file.clone(),
            expected: manifest.image_sha256.clone(),
            actual,
        });
    }
    decode_rgba_png(&path, &bytes, manifest.width, manifest.height)
}

/// Compare one candidate readback against a backend-bound golden.
///
/// Check order is part of the contract: backend binding fails before
/// placeholder status, environment drift fails before dimensions, and
/// dimensions fail before any pixel is diffed.
pub fn compare_readback(
    manifest: &GoldenManifest,
    host: &HostBinding,
    golden_rgba: &[u8],
    candidate: &Readback,
) -> Result<(), GoldenError> {
    manifest.validate()?;
    if manifest.backend != host.backend {
        return Err(GoldenError::BackendMismatch {
            golden: manifest.backend.clone(),
            host: host.backend.clone(),
        });
    }
    if manifest.status != GOLDEN_STATUS_CAPTURED {
        return Err(GoldenError::PlaceholderGolden {
            backend: manifest.backend.clone(),
        });
    }
    let drift_checks: [(&'static str, &str, &str); 4] = [
        ("os", &manifest.os, &host.os),
        ("adapter", &manifest.adapter, &host.adapter),
        (
            "atlas_manifest_sha256",
            &manifest.atlas_manifest_sha256,
            &host.atlas_manifest_sha256,
        ),
        (
            "shader_canonical_sha256",
            &manifest.shader_canonical_sha256,
            &host.shader_canonical_sha256,
        ),
    ];
    for (field, golden, host_value) in drift_checks {
        if golden != host_value {
            return Err(GoldenError::ManifestDrift {
                field,
                golden: golden.into(),
                host: host_value.into(),
            });
        }
    }
    if (manifest.width, manifest.height) != (candidate.width, candidate.height) {
        return Err(GoldenError::DimensionMismatch {
            golden_width: manifest.width,
            golden_height: manifest.height,
            got_width: candidate.width,
            got_height: candidate.height,
        });
    }
    let expected_len = (manifest.width as usize) * (manifest.height as usize) * 4;
    if golden_rgba.len() != expected_len {
        return Err(GoldenError::ImageSizeMismatch {
            expected: expected_len,
            got: golden_rgba.len(),
        });
    }
    if candidate.rgba.len() != expected_len {
        return Err(GoldenError::ImageSizeMismatch {
            expected: expected_len,
            got: candidate.rgba.len(),
        });
    }
    for (i, (&g, &c)) in golden_rgba.iter().zip(candidate.rgba.iter()).enumerate() {
        let delta = g.abs_diff(c);
        if delta > manifest.max_channel_delta {
            let pixel = (i / 4) as u32;
            return Err(GoldenError::DeltaAboveTolerance {
                x: pixel % manifest.width,
                y: pixel / manifest.width,
                channel: i % 4,
                golden: g,
                candidate: c,
                delta,
                max: manifest.max_channel_delta,
            });
        }
    }
    Ok(())
}

/// Machine-readable summary of one golden-vs-candidate comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldenDiffSummary {
    pub width: u32,
    pub height: u32,
    /// Tolerance the comparison ran at (the manifest's `max_channel_delta`).
    pub tolerance: u8,
    /// Pixels with at least one channel above tolerance.
    pub differing_pixels: u64,
    /// Largest single-channel delta anywhere in the frame.
    pub max_channel_delta: u8,
    /// `[x, y]` of the first differing pixel in raster order, if any.
    pub first_difference: Option<[u32; 2]>,
}

impl GoldenDiffSummary {
    pub fn is_drift(&self) -> bool {
        self.differing_pixels > 0
    }
}

/// Diff a candidate readback against golden pixels.
///
/// Returns the summary plus a human-reviewable RGBA mask: magenta where a
/// pixel exceeded `tolerance`, black where it did not. Dimensions are checked
/// first — a size mismatch has no per-pixel diff.
pub fn diff_readback(
    golden_rgba: &[u8],
    candidate: &Readback,
    tolerance: u8,
) -> Result<(GoldenDiffSummary, Vec<u8>), GoldenError> {
    let expected = (candidate.width as usize) * (candidate.height as usize) * 4;
    if golden_rgba.len() != expected {
        return Err(GoldenError::ImageSizeMismatch {
            expected,
            got: golden_rgba.len(),
        });
    }
    if candidate.rgba.len() != expected {
        return Err(GoldenError::ImageSizeMismatch {
            expected,
            got: candidate.rgba.len(),
        });
    }

    let pixels = (candidate.width as usize) * (candidate.height as usize);
    let mut mask = vec![0u8; pixels * 4];
    let mut differing_pixels = 0u64;
    let mut max_channel_delta = 0u8;
    let mut first_difference = None;

    for p in 0..pixels {
        let base = p * 4;
        let mut worst = 0u8;
        for c in 0..4 {
            worst = worst.max(golden_rgba[base + c].abs_diff(candidate.rgba[base + c]));
        }
        max_channel_delta = max_channel_delta.max(worst);
        let marked = worst > tolerance;
        if marked {
            differing_pixels += 1;
            if first_difference.is_none() {
                let index = p as u32;
                first_difference = Some([index % candidate.width, index / candidate.width]);
            }
        }
        mask[base..base + 4].copy_from_slice(if marked { &DIFF_MARK } else { &DIFF_KEEP });
    }

    Ok((
        GoldenDiffSummary {
            width: candidate.width,
            height: candidate.height,
            tolerance,
            differing_pixels,
            max_channel_delta,
            first_difference,
        },
        mask,
    ))
}

/// Write a reviewable drift artifact into `dir`: the candidate frame, the
/// per-pixel mask, and a JSON summary.
///
/// The artifact is *evidence*, never a tracked asset — callers point this at a
/// build directory. Writing it is what makes a golden failure actionable
/// instead of "some pixel somewhere moved".
///
/// A frame within tolerance writes **nothing** (not even `dir`) and returns its
/// summary: the artifact's presence is itself the signal that something drifted.
pub fn write_golden_diff(
    dir: &Path,
    golden_rgba: &[u8],
    candidate: &Readback,
    tolerance: u8,
) -> Result<GoldenDiffSummary, GoldenError> {
    let (summary, mask) = diff_readback(golden_rgba, candidate, tolerance)?;
    if !summary.is_drift() {
        return Ok(summary);
    }
    std::fs::create_dir_all(dir).map_err(|source| GoldenError::Io {
        path: dir.display().to_string(),
        source,
    })?;

    let actual_png = encode_rgba_png(candidate.width, candidate.height, &candidate.rgba)?;
    write_file(&dir.join(GOLDEN_DIFF_ACTUAL_PNG), &actual_png)?;
    let mask_png = encode_rgba_png(candidate.width, candidate.height, &mask)?;
    write_file(&dir.join(GOLDEN_DIFF_MASK_PNG), &mask_png)?;
    let summary_path = dir.join(GOLDEN_DIFF_SUMMARY_JSON);
    let json =
        serde_json::to_string_pretty(&summary).map_err(|source| GoldenError::ManifestJson {
            path: summary_path.display().to_string(),
            source,
        })?;
    write_file(&summary_path, (json + "\n").as_bytes())?;
    Ok(summary)
}

/// [`compare_readback`], but pixel drift also writes a diff artifact into
/// `diff_dir` and reports [`GoldenError::GoldenDrift`] naming it.
///
/// Only pixel-level drift produces an artifact. Backend mismatch, placeholder
/// status, environment drift and dimension mismatch propagate untouched —
/// there is no meaningful image diff for "this golden does not apply here",
/// and writing one would suggest the frame was compared when it was not.
pub fn compare_readback_writing_diff(
    manifest: &GoldenManifest,
    host: &HostBinding,
    golden_rgba: &[u8],
    candidate: &Readback,
    diff_dir: &Path,
) -> Result<(), GoldenError> {
    match compare_readback(manifest, host, golden_rgba, candidate) {
        Ok(()) => Ok(()),
        Err(GoldenError::DeltaAboveTolerance { .. }) => {
            // The drift verdict is the finding; a failed artifact write must not
            // replace it with an IO error that reads like an infrastructure
            // problem rather than a render regression.
            let (summary, artifact_dir) = match write_golden_diff(
                diff_dir,
                golden_rgba,
                candidate,
                manifest.max_channel_delta,
            ) {
                Ok(summary) => (summary, diff_dir.display().to_string()),
                Err(write_err) => {
                    let (summary, _) =
                        diff_readback(golden_rgba, candidate, manifest.max_channel_delta)?;
                    (
                        summary,
                        format!("<not written: {write_err}> ({})", diff_dir.display()),
                    )
                }
            };
            Err(GoldenError::GoldenDrift {
                differing_pixels: summary.differing_pixels,
                total_pixels: u64::from(summary.width) * u64::from(summary.height),
                max_channel_delta: summary.max_channel_delta,
                tolerance: summary.tolerance,
                artifact_dir,
            })
        }
        Err(other) => Err(other),
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), GoldenError> {
    std::fs::write(path, bytes).map_err(|source| GoldenError::Io {
        path: path.display().to_string(),
        source,
    })
}

/// Verify a benchmark report is bound to this golden's backend + atlas identity.
///
/// The report is untrusted candidate evidence; this rebinds its claimed
/// backend/atlas pins to the reviewed golden manifest.
pub fn verify_report_binding(
    manifest: &GoldenManifest,
    report: &crate::bench::ReportManifests,
) -> Result<(), GoldenError> {
    manifest.validate()?;
    if report.backend != manifest.backend {
        return Err(GoldenError::ReportBindingMismatch {
            field: "backend",
            golden: manifest.backend.clone(),
            report: report.backend.clone(),
        });
    }
    if report.atlas_manifest_sha256 != manifest.atlas_manifest_sha256 {
        return Err(GoldenError::ReportBindingMismatch {
            field: "atlas_manifest_sha256",
            golden: manifest.atlas_manifest_sha256.clone(),
            report: report.atlas_manifest_sha256.clone(),
        });
    }
    Ok(())
}

/// Compute current host workload pins for [`HostBinding`]:
/// `(atlas_manifest_sha256, shader_canonical_sha256)`.
pub fn host_binding_hashes(workspace_root: &Path) -> Result<(String, String), GoldenError> {
    let atlas_path = workspace_root.join("assets/sprites/generated/manifest.json");
    let atlas_bytes = std::fs::read(&atlas_path).map_err(|source| GoldenError::Io {
        path: atlas_path.display().to_string(),
        source,
    })?;
    let shader_path = workspace_root.join("shaders/generated/manifest.json");
    let shader_bytes = std::fs::read(&shader_path).map_err(|source| GoldenError::Io {
        path: shader_path.display().to_string(),
        source,
    })?;
    let shader_manifest: serde_json::Value =
        serde_json::from_slice(&shader_bytes).map_err(|source| GoldenError::ManifestJson {
            path: shader_path.display().to_string(),
            source,
        })?;
    let canonical = shader_manifest
        .get("canonical_sha256")
        .and_then(|v| v.as_str())
        .ok_or_else(|| GoldenError::ManifestField {
            path: shader_path.display().to_string(),
            field: "canonical_sha256",
        })?;
    Ok((sha256_hex(&atlas_bytes), canonical.to_string()))
}

/// Decode an untrusted candidate readback PNG into a [`Readback`].
///
/// `label` names the evidence source in errors (path or fixture id). The
/// bytes are candidate evidence: dimensions and format are enforced here,
/// pixel truth is established by [`compare_readback`] against the golden.
pub fn decode_readback_png(
    label: &str,
    bytes: &[u8],
    width: u32,
    height: u32,
) -> Result<Readback, GoldenError> {
    let rgba = decode_rgba_png(Path::new(label), bytes, width, height)?;
    Ok(Readback {
        width,
        height,
        rgba,
    })
}

/// Encode tightly-packed RGBA8 pixels as PNG bytes (deterministic settings).
pub fn encode_rgba_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, GoldenError> {
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(GoldenError::ImageSizeMismatch {
            expected,
            got: rgba.len(),
        });
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fastest);
        encoder.set_filter(png::Filter::NoFilter);
        let mut writer = encoder.write_header().map_err(|e| GoldenError::Png {
            path: "<encode>".into(),
            message: e.to_string(),
        })?;
        writer
            .write_image_data(rgba)
            .map_err(|e| GoldenError::Png {
                path: "<encode>".into(),
                message: e.to_string(),
            })?;
    }
    Ok(out)
}

/// Write a captured golden (`image_file` + `manifest.json`) into `dir`,
/// filling `image_sha256` from the encoded PNG bytes.
pub fn write_golden(
    dir: &Path,
    manifest: &mut GoldenManifest,
    rgba: &[u8],
) -> Result<(), GoldenError> {
    let png = encode_rgba_png(manifest.width, manifest.height, rgba)?;
    manifest.image_sha256 = sha256_hex(&png);
    manifest.status = GOLDEN_STATUS_CAPTURED.into();
    manifest.validate()?;
    std::fs::create_dir_all(dir).map_err(|source| GoldenError::Io {
        path: dir.display().to_string(),
        source,
    })?;
    let image_path = dir.join(&manifest.image_file);
    std::fs::write(&image_path, &png).map_err(|source| GoldenError::Io {
        path: image_path.display().to_string(),
        source,
    })?;
    let manifest_path = dir.join("manifest.json");
    let json =
        serde_json::to_string_pretty(manifest).map_err(|source| GoldenError::ManifestJson {
            path: manifest_path.display().to_string(),
            source,
        })?;
    std::fs::write(&manifest_path, json + "\n").map_err(|source| GoldenError::Io {
        path: manifest_path.display().to_string(),
        source,
    })?;
    Ok(())
}

fn decode_rgba_png(
    path: &Path,
    bytes: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, GoldenError> {
    let png_err = |message: String| GoldenError::Png {
        path: path.display().to_string(),
        message,
    };
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|e| png_err(e.to_string()))?;
    let info = reader.info();
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(png_err(format!(
            "expected RGBA8, got {:?}/{:?}",
            info.color_type, info.bit_depth
        )));
    }
    if (info.width, info.height) != (width, height) {
        return Err(GoldenError::DimensionMismatch {
            golden_width: width,
            golden_height: height,
            got_width: info.width,
            got_height: info.height,
        });
    }
    let mut buf = vec![
        0u8;
        reader
            .output_buffer_size()
            .ok_or_else(|| png_err("png output buffer size overflow".into()))?
    ];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|e| png_err(e.to_string()))?;
    buf.truncate(frame.buffer_size());
    let expected = (width as usize) * (height as usize) * 4;
    if buf.len() != expected {
        return Err(GoldenError::ImageSizeMismatch {
            expected,
            got: buf.len(),
        });
    }
    Ok(buf)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}
