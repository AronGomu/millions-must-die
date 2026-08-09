# Manual true-GPU profiler capture runbook (T13)

`gpu_queue_latency` in benchmark reports is a submit-to-fence proxy, **not** true GPU
execution time. When a gate misses or output drifts, capture a real frame in a native
GPU profiler and read actual pass/draw timings. This runbook is operator-facing;
captures are reviewed evidence, never automated gate inputs.

Scene under capture: the offscreen static demo (`static-demo-v1`, 1920x1080, four
atlas draw groups) — the same scene bound by `lab/goldens/<family>/manifest.json`.

## Status matrix

| Family | Tool | Status |
| --- | --- | --- |
| linux-vulkan | RenderDoc | verified locally (this runbook) |
| windows-d3d12 | RenderDoc | documented, unverified — `deferred-hw` (no Windows ref PC) |
| macos-metal | Xcode Metal debugger | documented, unverified — `deferred-hw` (no M4 Mac) |

## Linux / Vulkan — RenderDoc

Prereqs: RenderDoc >= 1.30 (`renderdoccmd` + `qrenderdoc`), the host Vulkan ICD used
for golden capture (adapter must match `lab/goldens/linux-vulkan/manifest.json`).

1. Build the golden test binary with debug info (renderer is created with
   `debug_mode = true`, enabling SDL GPU validation naming):

   ```bash
   cargo test -p mmd-engine --test gpu_golden --no-run
   ls target/debug/deps/gpu_golden-*   # note the executable (no .d suffix)
   ```

2. Launch it under RenderDoc from the workspace root so atlases/goldens resolve.
   This renders exactly `static-demo-v1` through the offscreen + readback path the
   golden binds:

   ```bash
   renderdoccmd capture -w -c /tmp/mmd-cap --working-dir . \
     target/debug/deps/gpu_golden-<hash> --ignored --test-threads=1 \
     host_offscreen_matches_tracked_golden
   ```

   (Or launch via `qrenderdoc` → Launch Application with the same executable,
   arguments, and working directory.)

   Alternative — interactive horde: `target/debug/millions_must_die run --agents
   5000` captures the same pipeline/resolution/atlas groups, but the scene is the
   moving flow-field horde and the frame ends in a swapchain blit instead of the
   transfer-buffer readback; do not use it to reason about the golden scene.
3. Trigger a capture with `F12` (or `renderdoccmd`'s auto-capture flags). For the
   interactive alternative, capture once the scene is stable — after warmup, not
   on the first frames.
4. Open the `.rdc` in `qrenderdoc` and verify the frame shape before trusting timings:
   - Event Browser shows the offscreen color pass at 1920x1080 followed by the
     readback copy (offscreen texture → transfer buffer). In the interactive
     alternative the pass is followed by a swapchain blit instead.
   - Four instanced indexed draws (one per atlas group) inside the pass; instance
     counts match the agent count split.
   - Bound pipeline uses the SPIR-V blobs pinned in `shaders/generated/manifest.json`.
5. Read true GPU timings: Event Browser → clock icon ("Time durations for actions").
   Record per-pass and per-draw GPU durations. These are the true-GPU numbers that
   `gpu_queue_latency` only approximates.
6. Archive evidence: keep the `.rdc` plus a note of commit SHA, driver version, and
   adapter (`vulkaninfo --summary`). Store outside the candidate tree; captures are
   reviewed evidence for recalibration decisions (e.g., any future golden tolerance
   change requires this kind of native evidence).

Interference note: capture runs are **not** benchmark runs. Never record gate
timings while RenderDoc is attached — the overlay/instrumentation skews frame times.

## Windows / D3D12 — RenderDoc (`deferred-hw`, documented-but-unverified)

> deferred-hw (2026-08-05): no Windows ref PC exists. Procedure below is written from
> RenderDoc's documented D3D12 support and must be verified on the real RX 6400 host
> before being relied on.

1. Install RenderDoc for Windows; build the golden test binary from the exact commit
   (`cargo test -p mmd-engine --test gpu_golden --no-run`).
2. In `qrenderdoc` → Launch Application: the `gpu_golden-<hash>.exe` under
   `target\debug\deps`, arguments `--ignored --test-threads=1
   host_offscreen_matches_tracked_golden`, working directory = workspace root.
3. Capture with `F12`; confirm the D3D12 device (not WARP / Basic Render Driver —
   adapter must match `lab/goldens/windows-d3d12/manifest.json` once captured).
4. Verify frame shape (offscreen pass, four instanced draws, DXIL shaders per
   `shaders/generated/manifest.json`) and read GPU durations as on Linux.

## macOS / Metal — Xcode Metal debugger (`deferred-hw`, documented-but-unverified)

> deferred-hw (2026-08-05): no M4 Mac exists. Procedure below is written from Apple's
> documented Metal debugger workflow and must be verified on the real Apple Silicon
> host before being relied on.

1. Build the golden test binary on the Mac (`cargo test -p mmd-engine --test
   gpu_golden --no-run`); create an Xcode scheme wrapping the `gpu_golden-<hash>`
   executable under `target/debug/deps` (Product → Scheme → Edit Scheme → Run →
   Info → select the binary; set arguments `--ignored --test-threads=1
   host_offscreen_matches_tracked_golden` and working directory = workspace root).
   Enable Options → GPU Frame Capture → Metal.
2. Run, then click the Metal camera icon (Capture GPU Workload).
3. In the capture: verify one render command encoder targeting the 1920x1080
   offscreen texture, four instanced draws, `metallib` functions per
   `shaders/generated/manifest.json`, and the blit/readback encoder.
4. Use Performance → GPU timeline for per-encoder/per-draw GPU durations
   (Counters require a device profile; M-series exposes stage timings).

## Relationship to goldens

Profiler captures and golden compares are two views of the same reviewed scene:

- Goldens (`crates/mmd-engine/src/render/golden.rs`, `cargo test -p mmd-engine
  --test gpu_golden -- --ignored --test-threads=1`) prove output **correctness** —
  exact, backend-bound, manifest-pinned.
- Profiler captures prove **where GPU time goes** when perf gates fail.

Both bind to the same manifest pins (adapter, atlas manifest hash, shader canonical
hash); if a driver/adapter changes, the golden comparator blocks with a
recalibration error and any new capture/golden must be re-reviewed together.

## Golden regeneration (reviewed, Linux-local)

```bash
MMD_UPDATE_GOLDEN=1 cargo test -p mmd-engine --test gpu_golden -- \
  --ignored --test-threads=1 update_host_golden
git diff --stat lab/goldens/   # review manifest + image diff before committing
```

Windows/macOS golden capture: run the same command on the native host once hardware
exists (`[deferred-hw]`); it writes the host's own family directory only.
