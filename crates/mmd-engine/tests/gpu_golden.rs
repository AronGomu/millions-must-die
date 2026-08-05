//! Backend-bound golden correctness (T13).
//!
//! Pure comparator tests always run. `#[ignore]` cases need a real GPU + SDL3
//! and share process-global SDL; run serially (`--test-threads=1`).
//!
//! Regenerate the host golden (review the diff before committing):
//! `MMD_UPDATE_GOLDEN=1 cargo test -p mmd-engine --test gpu_golden -- --ignored update_host_golden`

use std::path::PathBuf;

use mmd_engine::bench::ReportManifests;
use mmd_engine::render::{
    BACKEND_D3D12, BACKEND_METAL, BACKEND_VULKAN, GOLDEN_MAX_CHANNEL_DELTA_POLICY,
    GOLDEN_SCENE_STATIC_DEMO, GOLDEN_SCHEMA_VERSION, GOLDEN_STATUS_CAPTURED,
    GOLDEN_STATUS_PLACEHOLDER, GoldenError, GoldenManifest, HostBinding, REQUIRED_BACKEND,
    Readback, SpriteRenderer, VIEW_HEIGHT, VIEW_WIDTH, compare_readback, encode_rgba_png,
    golden_family_dir, host_binding_hashes, load_golden_image, load_golden_manifest,
    verify_report_binding, write_golden,
};
use mmd_engine::workspace_root;

const W: u32 = 8;
const H: u32 = 4;

fn synth_manifest(backend: &str, os: &str) -> GoldenManifest {
    GoldenManifest {
        schema_version: GOLDEN_SCHEMA_VERSION.into(),
        backend: backend.into(),
        os: os.into(),
        status: GOLDEN_STATUS_CAPTURED.into(),
        scene: GOLDEN_SCENE_STATIC_DEMO.into(),
        width: W,
        height: H,
        image_file: "golden.png".into(),
        image_sha256: String::new(),
        adapter: "Fake Adapter 1000".into(),
        driver_info: "fake-driver 1.2.3".into(),
        atlas_manifest_sha256: "aa".repeat(32),
        shader_canonical_sha256: "bb".repeat(32),
        max_channel_delta: 0,
    }
}

fn synth_host(m: &GoldenManifest) -> HostBinding {
    HostBinding {
        backend: m.backend.clone(),
        os: m.os.clone(),
        adapter: m.adapter.clone(),
        atlas_manifest_sha256: m.atlas_manifest_sha256.clone(),
        shader_canonical_sha256: m.shader_canonical_sha256.clone(),
    }
}

fn synth_pixels(w: u32, h: u32) -> Vec<u8> {
    (0..(w * h * 4)).map(|i| (i % 251) as u8).collect()
}

fn synth_readback(w: u32, h: u32, rgba: Vec<u8>) -> Readback {
    Readback {
        width: w,
        height: h,
        rgba,
    }
}

fn goldens_root() -> PathBuf {
    workspace_root().join("lab/goldens")
}

#[test]
fn dimension_mismatch_fails() {
    let manifest = synth_manifest(BACKEND_VULKAN, "linux");
    let host = synth_host(&manifest);
    let golden = synth_pixels(W, H);
    let candidate = synth_readback(W, H + 1, synth_pixels(W, H + 1));
    let err = compare_readback(&manifest, &host, &golden, &candidate).expect_err("dims");
    assert!(
        matches!(err, GoldenError::DimensionMismatch { .. }),
        "{err}"
    );
}

#[test]
fn manifest_mismatch_fails() {
    // Driver/adapter drift must block with a recalibration error, not diff pixels.
    let manifest = synth_manifest(BACKEND_VULKAN, "linux");
    let mut host = synth_host(&manifest);
    host.adapter = "Different GPU 2000".into();
    let golden = synth_pixels(W, H);
    let candidate = synth_readback(W, H, golden.clone());
    let err = compare_readback(&manifest, &host, &golden, &candidate).expect_err("drift");
    assert!(matches!(err, GoldenError::ManifestDrift { .. }), "{err}");
    assert!(
        err.to_string().to_lowercase().contains("recalibrat"),
        "{err}"
    );

    // Shader canonical drift blocks the same way.
    let mut host2 = synth_host(&manifest);
    host2.shader_canonical_sha256 = "cc".repeat(32);
    let err2 = compare_readback(&manifest, &host2, &golden, &candidate).expect_err("shader drift");
    assert!(matches!(err2, GoldenError::ManifestDrift { .. }), "{err2}");
}

#[test]
fn delta_above_tolerance_fails() {
    let manifest = synth_manifest(BACKEND_VULKAN, "linux");
    let host = synth_host(&manifest);
    let golden = synth_pixels(W, H);
    let mut drifted = golden.clone();
    drifted[13] = drifted[13].wrapping_add(3);
    let candidate = synth_readback(W, H, drifted);
    let err = compare_readback(&manifest, &host, &golden, &candidate).expect_err("delta");
    assert!(
        matches!(err, GoldenError::DeltaAboveTolerance { .. }),
        "{err}"
    );

    // Exact policy: even a single ±1 channel step fails until reviewed native
    // evidence justifies a bounded tolerance.
    let mut one_off = golden.clone();
    one_off[0] = one_off[0].wrapping_add(1);
    let candidate = synth_readback(W, H, one_off);
    assert!(compare_readback(&manifest, &host, &golden, &candidate).is_err());
}

#[test]
fn backend_cannot_use_other_golden() {
    // Synthetic: Metal golden on a Vulkan host binding.
    let manifest = synth_manifest(BACKEND_METAL, "macos");
    let mut host = synth_host(&manifest);
    host.backend = BACKEND_VULKAN.into();
    host.os = "linux".into();
    let golden = synth_pixels(W, H);
    let candidate = synth_readback(W, H, golden.clone());
    let err = compare_readback(&manifest, &host, &golden, &candidate).expect_err("backend");
    assert!(matches!(err, GoldenError::BackendMismatch { .. }), "{err}");

    // Real tracked fixtures: every non-host golden family must be rejected
    // before any pixel or placeholder logic runs.
    for family in ["linux-vulkan", "windows-d3d12", "macos-metal"] {
        let manifest =
            load_golden_manifest(&goldens_root().join(family).join("manifest.json")).expect(family);
        if manifest.backend == REQUIRED_BACKEND {
            continue;
        }
        let host = HostBinding {
            backend: REQUIRED_BACKEND.into(),
            os: std::env::consts::OS.into(),
            adapter: "any".into(),
            atlas_manifest_sha256: manifest.atlas_manifest_sha256.clone(),
            shader_canonical_sha256: manifest.shader_canonical_sha256.clone(),
        };
        let candidate = synth_readback(manifest.width, manifest.height, vec![]);
        let err = compare_readback(&manifest, &host, &[], &candidate).expect_err("foreign golden");
        assert!(
            matches!(err, GoldenError::BackendMismatch { .. }),
            "{family}: {err}"
        );
    }
}

#[test]
fn exact_image_passes() {
    let manifest = synth_manifest(BACKEND_VULKAN, "linux");
    let host = synth_host(&manifest);
    let golden = synth_pixels(W, H);
    let candidate = synth_readback(W, H, golden.clone());
    compare_readback(&manifest, &host, &golden, &candidate).expect("exact match passes");
}

#[test]
fn tolerance_above_policy_is_rejected() {
    // Widening tolerance requires reviewed native evidence; a manifest alone
    // must not be able to loosen the gate.
    let mut manifest = synth_manifest(BACKEND_VULKAN, "linux");
    manifest.max_channel_delta = GOLDEN_MAX_CHANNEL_DELTA_POLICY + 1;
    let host = synth_host(&manifest);
    let golden = synth_pixels(W, H);
    let candidate = synth_readback(W, H, golden.clone());
    let err = compare_readback(&manifest, &host, &golden, &candidate).expect_err("tolerance");
    assert!(
        matches!(err, GoldenError::ToleranceAbovePolicy { .. }),
        "{err}"
    );
}

#[test]
fn placeholder_golden_cannot_pass() {
    // deferred-hw placeholder manifests document intent but never compare.
    let mut manifest = synth_manifest(BACKEND_D3D12, "windows");
    manifest.status = GOLDEN_STATUS_PLACEHOLDER.into();
    let host = synth_host(&manifest);
    let golden = synth_pixels(W, H);
    let candidate = synth_readback(W, H, golden.clone());
    let err = compare_readback(&manifest, &host, &golden, &candidate).expect_err("placeholder");
    assert!(
        matches!(err, GoldenError::PlaceholderGolden { .. }),
        "{err}"
    );
}

#[test]
fn tampered_golden_image_fails_hash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manifest = synth_manifest(BACKEND_VULKAN, "linux");
    let pixels = synth_pixels(W, H);
    write_golden(dir.path(), &mut manifest, &pixels).expect("write golden");

    // Sanity: untampered golden round-trips.
    let loaded = load_golden_image(dir.path(), &manifest).expect("load");
    assert_eq!(loaded, pixels);

    // Flip one byte of the tracked PNG: hash gate must fail before decode use.
    let png_path = dir.path().join(&manifest.image_file);
    let mut bytes = std::fs::read(&png_path).expect("read png");
    let last = bytes.last_mut().expect("png bytes");
    *last ^= 1;
    std::fs::write(&png_path, bytes).expect("tamper png");
    let err = load_golden_image(dir.path(), &manifest).expect_err("tampered");
    assert!(
        matches!(err, GoldenError::ImageHashMismatch { .. }),
        "{err}"
    );
}

#[test]
fn golden_family_dirs_are_backend_bound() {
    assert_eq!(
        golden_family_dir(BACKEND_VULKAN).expect("vk"),
        "linux-vulkan"
    );
    assert_eq!(
        golden_family_dir(BACKEND_D3D12).expect("dx"),
        "windows-d3d12"
    );
    assert_eq!(
        golden_family_dir(BACKEND_METAL).expect("mtl"),
        "macos-metal"
    );
    assert!(golden_family_dir("software").is_err());
}

#[test]
fn tracked_golden_fixtures_are_valid() {
    // Repo-tracked golden manifests stay well-formed on every host.
    let root = goldens_root();
    let expect = [
        ("linux-vulkan", BACKEND_VULKAN, "linux"),
        ("windows-d3d12", BACKEND_D3D12, "windows"),
        ("macos-metal", BACKEND_METAL, "macos"),
    ];
    for (family, backend, os) in expect {
        let manifest =
            load_golden_manifest(&root.join(family).join("manifest.json")).expect(family);
        assert_eq!(manifest.backend, backend, "{family}");
        assert_eq!(manifest.os, os, "{family}");
        assert_eq!(manifest.schema_version, GOLDEN_SCHEMA_VERSION, "{family}");
        assert_eq!(manifest.scene, GOLDEN_SCENE_STATIC_DEMO, "{family}");
        assert_eq!(
            manifest.max_channel_delta, GOLDEN_MAX_CHANNEL_DELTA_POLICY,
            "{family}"
        );
        if family == "linux-vulkan" {
            // Captured golden: image tracked, hash-bound, gate resolution.
            assert_eq!(manifest.status, GOLDEN_STATUS_CAPTURED, "{family}");
            assert_eq!((manifest.width, manifest.height), (VIEW_WIDTH, VIEW_HEIGHT));
            let pixels = load_golden_image(&root.join(family), &manifest).expect("hash-bound");
            assert_eq!(
                pixels.len(),
                (manifest.width * manifest.height * 4) as usize
            );
        } else {
            // Native capture deferred: placeholder never carries an image.
            assert_eq!(manifest.status, GOLDEN_STATUS_PLACEHOLDER, "{family}");
            assert!(manifest.image_sha256.is_empty(), "{family}");
        }
    }
}

#[test]
fn report_binding_enforced() {
    let manifest = synth_manifest(BACKEND_VULKAN, "linux");
    let mut report = ReportManifests {
        scenario_version: "technical_prototype_v1".into(),
        scenario_sha256: "dd".repeat(32),
        atlas_manifest_sha256: manifest.atlas_manifest_sha256.clone(),
        backend: manifest.backend.clone(),
        adapter: manifest.adapter.clone(),
        shader_manifest_version: 1,
        engine_version: "0.0.0".into(),
        policy_id: "test".into(),
        frames_in_flight: 2,
        gpu_queue_latency_note: String::new(),
        project_alloc_visibility_note: String::new(),
    };
    verify_report_binding(&manifest, &report).expect("bound report passes");

    // Backend mismatch: a report from another backend can't claim this golden.
    report.backend = BACKEND_METAL.into();
    let err = verify_report_binding(&manifest, &report).expect_err("backend");
    assert!(
        matches!(err, GoldenError::ReportBindingMismatch { .. }),
        "{err}"
    );

    // Atlas drift: report rendered against different atlas bytes is unbound.
    report.backend = manifest.backend.clone();
    report.atlas_manifest_sha256 = "ee".repeat(32);
    let err = verify_report_binding(&manifest, &report).expect_err("atlas");
    assert!(
        matches!(err, GoldenError::ReportBindingMismatch { .. }),
        "{err}"
    );
}

#[test]
fn png_roundtrip_is_lossless() {
    let pixels = synth_pixels(W, H);
    let png = encode_rgba_png(W, H, &pixels).expect("encode");
    assert!(!png.is_empty());
    let dir = tempfile::tempdir().expect("tempdir");
    let mut manifest = synth_manifest(BACKEND_VULKAN, "linux");
    write_golden(dir.path(), &mut manifest, &pixels).expect("write");
    let loaded = load_golden_image(dir.path(), &manifest).expect("load");
    assert_eq!(loaded, pixels);
}

// ---------------------------------------------------------------------------
// Native GPU cases (`--ignored`, serial).
// ---------------------------------------------------------------------------

fn live_host_binding(renderer: &SpriteRenderer) -> HostBinding {
    let (atlas_sha, shader_sha) = host_binding_hashes(&workspace_root()).expect("host hashes");
    HostBinding {
        backend: renderer.backend().to_string(),
        os: std::env::consts::OS.into(),
        adapter: renderer.ctx.adapter.clone(),
        atlas_manifest_sha256: atlas_sha,
        shader_canonical_sha256: shader_sha,
    }
}

/// Regenerates the host-backend golden. Guarded by `MMD_UPDATE_GOLDEN=1`;
/// output is a reviewed artifact — inspect the diff before committing.
#[test]
#[ignore = "requires host GPU + SDL3; writes lab/goldens when MMD_UPDATE_GOLDEN=1"]
fn update_host_golden() {
    if std::env::var("MMD_UPDATE_GOLDEN").as_deref() != Ok("1") {
        eprintln!("MMD_UPDATE_GOLDEN != 1; skipping golden regeneration");
        return;
    }
    let mut renderer = SpriteRenderer::new(&workspace_root(), true).expect("renderer");
    let host = live_host_binding(&renderer);
    let rb = renderer
        .draw_offscreen_readback(&SpriteRenderer::static_demo_groups())
        .expect("readback");
    let family = golden_family_dir(&host.backend).expect("family");
    let dir = goldens_root().join(family);
    let mut manifest = GoldenManifest {
        schema_version: GOLDEN_SCHEMA_VERSION.into(),
        backend: host.backend.clone(),
        os: host.os.clone(),
        status: GOLDEN_STATUS_CAPTURED.into(),
        scene: GOLDEN_SCENE_STATIC_DEMO.into(),
        width: rb.width,
        height: rb.height,
        image_file: "golden.png".into(),
        image_sha256: String::new(),
        adapter: host.adapter.clone(),
        driver_info: format!("captured via SDL3 GPU on {}", host.os),
        atlas_manifest_sha256: host.atlas_manifest_sha256.clone(),
        shader_canonical_sha256: host.shader_canonical_sha256.clone(),
        max_channel_delta: 0,
    };
    write_golden(&dir, &mut manifest, &rb.rgba).expect("write golden");
    eprintln!("golden written to {}", dir.display());
}

#[test]
#[ignore = "requires host GPU + SDL3"]
fn host_offscreen_matches_tracked_golden() {
    let mut renderer = SpriteRenderer::new(&workspace_root(), true).expect("renderer");
    let host = live_host_binding(&renderer);
    let family = golden_family_dir(&host.backend).expect("family");
    let dir = goldens_root().join(family);
    let manifest = load_golden_manifest(&dir.join("manifest.json")).expect("manifest");
    let golden = load_golden_image(&dir, &manifest).expect("golden image");
    let rb = renderer
        .draw_offscreen_readback(&SpriteRenderer::static_demo_groups())
        .expect("readback");
    compare_readback(&manifest, &host, &golden, &rb).expect("host output matches golden");
}

#[test]
#[ignore = "requires host GPU + SDL3"]
fn live_readback_rejects_foreign_golden() {
    let mut renderer = SpriteRenderer::new(&workspace_root(), true).expect("renderer");
    let host = live_host_binding(&renderer);
    let rb = renderer
        .draw_offscreen_readback(&SpriteRenderer::static_demo_groups())
        .expect("readback");
    // A live readback must not validate against any other backend's golden.
    for family in ["linux-vulkan", "windows-d3d12", "macos-metal"] {
        let manifest =
            load_golden_manifest(&goldens_root().join(family).join("manifest.json")).expect(family);
        if manifest.backend == host.backend {
            continue;
        }
        let err = compare_readback(&manifest, &host, &[], &rb).expect_err("foreign golden");
        assert!(
            matches!(err, GoldenError::BackendMismatch { .. }),
            "{family}: {err}"
        );
    }
}
