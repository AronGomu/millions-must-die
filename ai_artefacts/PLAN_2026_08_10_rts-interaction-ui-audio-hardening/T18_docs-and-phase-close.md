# T18: Close docs and run merge gate

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T17  
**Commit outcome:** phase1.1 docs/ADRs/architecture/test map describe exact landed code; full deterministic merge gate including audio generation passes.

## Context (self-contained)

- Goal: close phase1.1 on functional scope only; leave auditable proof/gaps; no stale phase1 constraints.
- This slice: docs/contracts/status + full gate. No new game behavior.
- Out of scope: benchmarks/perf claims, physical cross-platform claims, golden regeneration, copyrighted track.
- Assumptions: T17 exact test names/counters are final. Existing phase1 close remains historical truth and is not rewritten as if it included phase1.1.

## Requirements

- Change proposed ADRs 016–020 to `Status: Accepted`; add implementation consequences/corrections only if landed code differs; never rewrite decisions silently.
- Update `docs/ADR/README.md` accepted phase1.1 index + supersession map.
- Create `docs/rts-interaction-ui-audio-hardening-functional-close.md` with:
  - system→real test map;
  - exact 1,600-frame smoke + exit fields;
  - proof boundaries/known gaps/manual-only hardware checks;
  - no perf claim.
- Preserve `docs/rts-engine-prototype-functional-close.md`; add one forward link noting phase1.1 follows it, without changing historical claims.
- Update `AGENT.md`, root `README.md`, `docs/CONTEXT.md`, `docs/DESIGN.md`, `docs/05-testing.md`, `docs/README.md`, `docs/GLOSSARY.md`.
- Required new glossary words, unique/lowercase/exact refs: `pickshape`, `body`, `staticnav`, `formation`, `frontier`, `canvas`, `commandcard`, `minimap`, `windowmode`, `audiosink`, `audiobus`.
- Update architecture HTML created by planning: `docs/rts-interaction-ui-audio-hardening-architecture.html` from “Proposed” to “Implemented”; add exact file/test refs + known gaps. Update `docs/rts-engine-prototype-architecture.html` and `docs/agent-collision-architecture.html` with links/scoped contrast; do not claim horde hard collision.
- Add `cargo run -p xtask -- audio --check` to single-source required gate in `docs/05-testing.md`, mirrored in `AGENT.md`/README.
- Keep all existing gate commands unchanged. RTS smoke remains exactly 1,600 frames.
- Extend `tests/validation_contract.rs`:
  - ADR index lists 016–020;
  - every new link resolves;
  - phase1.1 close names only real tests;
  - system map includes pick/body/static nav/formation/settings/canvas/window/camera/HUD/minimap/audio;
  - no perf claim covers new live docs;
  - required gate contains audio check + exact RTS smoke.
- `cargo tree -e features | grep -c testkit` remains0 shipping.
- Do not regenerate phase-0 render golden.

## Inputs

- Landed T1–T17 code/tests/actual names.
- Proposed ADRs 016–020 + planned architecture page generated with this plan.
- `docs/05-testing.md` remains gate authority.
- **From Depends:** T17 exact smoke/counters/test names; copy actual names, never plan drafts if changed.

## TDD

1. **Red** — extend validation-contract expectations first; fail on missing docs/index/gate/test refs.
2. **Green** — update/create docs + ADR status/page links until validation passes.
3. **Refactor** — remove duplicate gate lists only where repo policy allows; keep `docs/05-testing.md` authority explicit.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `adr_index_lists_every_adr_file` | ADR dir/index | 001–020 listed |
| `every_doc_link_resolves` | updated docs | all links exist |
| `phase1_1_close_names_only_real_tests` | close test map | every named test resolves |
| `phase1_1_systems_have_behavioral_tests` | system map | all 11 systems mapped |
| `no_perf_claim_in_docs` | new docs/HTML text extraction | no live measured speed claim |
| `required_gate_contains_audio_check` | testing doc | exact xtask cmd |
| `required_gate_keeps_phase_smokes` | testing doc | all 4 existing smokes exact |
| shipping feature check | cargo tree | count0 |

## Impl steps

- [x] 1. Add red validation-contract cases for docs/ADRs/gate/system map. — validate: `cargo test -p millions_must_die --test validation_contract` fails naming the missing phase-1.1 docs/gate entries before any doc is written.
- [x] 2. Promote ADR 016–020 status; add actual consequences/corrections. — validate: `grep -c 'Status: Accepted' docs/ADR/01[6-9]*.md docs/ADR/020*.md` is 1 per file, each landed deviation (adaptive reach, push chain, slot arrival, `ProductionQueue` self-methods, Escape→pause menu) named in the owning record.
- [x] 3. Write phase1.1 functional close from landed evidence. — validate: `docs/rts-interaction-ui-audio-hardening-functional-close.md` exists and `phase1_1_close_names_only_real_tests` passes against it.
- [x] 4. Update status/design/testing/nav/glossary/root agent docs surgically. — validate: every required glossary word present with a resolvable code ref; `AGENT.md`/`README.md`/`docs/CONTEXT.md`/`docs/DESIGN.md`/`docs/README.md` name phase 1.1 and no longer state a superseded phase-1 constraint.
- [x] 5. Update planned/new + existing architecture HTML with exact implementation refs. — validate: phase-1.1 page says Implemented (no "NOT IMPLEMENTED"), all three HTML pages cross-link, `every_doc_link_resolves` passes.
- [x] 6. Add audio check to required gate mirrors; keep 1,600-frame RTS command. — validate: `required_gate_contains_audio_check` + `required_gate_keeps_phase_smokes` pass on `docs/05-testing.md` and `README.md`.
- [x] 7. Run doc/link/no-perf tests; inspect HTML manually. — validate: `cargo test -p millions_must_die --test validation_contract` green; each HTML page read end to end for stale "proposed" wording.
- [x] 8. Run entire merge gate exactly below; record no perf numbers. — validate: every Validation command below reports success and no command output is quoted as a speed number.

## Outputs

- New: phase1.1 functional-close Markdown.
- Modified: ADR statuses/index; AGENT/README/context/design/testing/docs nav/glossary; 3 architecture HTML pages; validation contract.
- Behavior: none; governance/docs now match shipped phase1.1.
- Config: merge gate adds deterministic audio asset check.

## Validation

- [x] `cargo fmt --all -- --check` — validate: exit 0, no diff printed.
- [x] `cargo test --workspace --locked` — validate: every binary reports `test result: ok`.
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings` — validate: exit 0, zero warnings.
- [x] `nix flake check` — validate: exit 0.
- [x] `cargo run -p xtask -- bootstrap --check` — validate: exit 0.
- [x] `cargo run -p xtask -- shaders --check` — validate: exit 0.
- [x] `cargo run -p xtask -- atlases --check` — validate: exit 0.
- [x] `cargo run -p xtask -- audio --check` — validate: prints `audio: ok (7 wav + manifest)`, exit 0.
- [x] `cargo run -- run --agents 5000 --frames 300` — validate: prints `run: clean exit`, exit 0.
- [x] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` — validate: prints `run: clean exit`, exit 0. (Run under `SDL_VIDEODRIVER=offscreen`: an agent may not open a window on this live desktop.)
- [x] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300` — validate: prints `run: clean exit`, exit 0. (Same offscreen constraint.)
- [x] `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` — validate: exit line ends `body_overlaps=0 ui_page=gameplay music_starts=1 voice_select=8 voice_order=9 voice_reject=1 sfx_ui=8 keyboard_pan=78`. (Run under `SDL_VIDEODRIVER=offscreen SDL_AUDIODRIVER=dummy`: the windowed form grabs the pointer and plays audio, so it stays a human checklist item.)
- [x] `test "$(cargo tree -e features | grep -c testkit)" -eq 0` — validate: exit 0.
- [x] manual check: open all 3 architecture HTML pages; verify diagrams/labels current — validated at source level (agent may not drive this desktop's browser): each page read in full, HTML parses, every relative `href`/`src` resolves, no "proposed/not implemented" label survives on the phase-1.1 page, cross-links present on the other two. Visual rendering stays a human checklist item (`T18` section of `ai_artefacts/manual_test_checklist.md`).
- [x] app functional: exact tested commit hash passes all commands; no golden regeneration — validate: `git status --porcelain lab/goldens` empty after the gate.
- [ ] commit msg draft: `docs(rts): close phase 1.1 on functional interaction scope` — validate: commit subject matches verbatim.
