# Manual test checklist

Steps a human must run and look at. One section per ticket; never edit another
ticket's section.

## T0 starcraft-scale-cap-and-body

Landed on `plan/horde-sim-headroom`. Everything automatable is green
(460 tests, clippy, `nix flake check`, all three xtask checks, all three
scene smokes). What is left needs eyes on a real window.

- [ ] `cargo run -- run --agents 5000` — a window opens and the horde moves.
      Units read as chunky StarCraft-scale sprites, clearly bigger than the old
      30 px units (the drawn quad is now 48 px). Press `Esc` to quit and confirm
      a clean exit.
- [ ] In that same window, watch two agents press together at a choke point or
      against an obstacle. Their sprites must meet **edge to edge**, not overlap
      — the body is exactly half a sprite (6 cells = 24 px radius), so contact
      happens at one full sprite width. Overlapping art means the body/sprite
      ratio is wrong.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` — the
      5 000-agent demo scene starts, spreads out of its spawn stacks, and quits
      cleanly on `Esc`.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      the 1 200-agent scene starts; at this lower density the edge-to-edge
      contact above is easiest to see. Quit with `Esc`.
- [ ] Judgement call for the plan owner, not a bug in this ticket: confirm
      5 000 agents at 48 px still reads as a horde rather than a sparse field.
      If it reads sparse, that is a design signal for the density knobs.

## T1 scenario-headroom-knobs

Contract-only slice: three new scenario fields (`separation_phases`,
`mass_class_count`, `separation_threads`), all pinned to their identity value
`1`. Nothing reads them yet, so a human should observe **zero visible change**
from T0. Everything automatable is green (`cargo fmt`, `cargo test --workspace
--locked`, clippy, `nix flake check`, `xtask bootstrap --check`, and all three
scene smokes with the gate hash reproduced byte for byte). What is left needs
eyes on a real window, same as T0.

- [ ] `cargo run -- run --agents 5000` — a window opens and the horde moves
      exactly as it did before this ticket: same 48 px sprites, same
      edge-to-edge contact at chokepoints. Press `Esc` to quit and confirm a
      clean exit.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` —
      starts, spreads, quits cleanly on `Esc`; behaviour indistinguishable
      from T0.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      same check at the lower-density scale.
- [ ] Try authoring a scenario `.ron` that omits `separation_phases`,
      `mass_class_count` or `separation_threads` (or sets one to `0`) and
      confirm it fails to load with a named error instead of running
      silently untuned — the new fields are required, not defaulted.

## T2 stamped-bin-counts

Internal neighbour-index rewrite (`SpatialGrid::rebuild` in
`crates/mmd-engine/src/sim/spatial.rs` no longer clears `starts` before
counting; bin population is now an O(1) `bin_count` query via a rebuild
stamp). Behaviour is bit-identical to before this ticket — a human should
observe **zero visible change**. Everything automatable is green (`cargo
fmt`, `cargo test --workspace --locked`, `cargo clippy --workspace
--all-targets --all-features -- -D warnings`, `nix flake check`, the
testkit-gated `cargo build -p mmd-engine --no-default-features --features
gpu`, and the gate smoke reproducing the T0-pinned digest
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
byte for byte). What is left needs eyes on a real window.

- [ ] `cargo run -- run --agents 5000` — a window opens and the horde moves
      exactly as it did before this ticket: same 48 px sprites, same
      edge-to-edge contact at chokepoints, same separation behaviour at
      chokepoints and in the corner-pocket case. Press `Esc` to quit and
      confirm a clean exit.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` —
      starts, spreads, quits cleanly on `Esc`; behaviour indistinguishable
      from T1.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      same check at the lower-density scale.

## T3 amortised-separation

The separation scan and grid rebuild now spread over the scenario's
`separation_phases` ticks. Every scene except `collision_mid_v1` still pins
`separation_phases: 1` and must look and behave exactly as before this
ticket. `collision_mid_v1` alone now amortises over 4 phases: at most a
3-tick-stale neighbour position, one grid rebuild per 4 ticks instead of
every tick. Everything automatable is green (`cargo fmt`, `cargo test
--workspace --locked`, `cargo clippy --workspace --all-targets
--all-features -- -D warnings`, `nix flake check`, the testkit-gated `cargo
build -p mmd-engine --no-default-features --features gpu`, and the gate
smoke reproducing the T0-pinned digest
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
byte for byte). What is left needs eyes on a real window.

- [ ] `cargo run -- run --agents 5000` — a window opens and the horde moves
      exactly as it did before this ticket (this scene stays at
      `separation_phases: 1`): same 48 px sprites, same edge-to-edge contact
      at chokepoints. Press `Esc` to quit and confirm a clean exit.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` —
      starts, the stacked spawn column must still visibly spread apart over
      the run (now amortised over 4 phases, so the spread may look very
      slightly slower/chunkier tick to tick, but must not stall or freeze),
      no agent stuck inside the scene's obstacle, quits cleanly on `Esc`.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      unaffected by this ticket (stays at `separation_phases: 1`); same check
      as T2 at the lower-density scale.

## T4 row-contiguous-scan

Pure restructuring of the per-agent 3x3 neighbour scan in
`crates/mmd-engine/src/sim/collision.rs`: it now fetches three row-contiguous
slices (`SpatialGrid::agents_in_bin_row`) instead of nine per-bin slices, and
an agent whose whole window holds only itself skips the scan and writes zero
directly. Behaviour is bit-identical to before this ticket — a human should
observe **zero visible change**. Everything automatable is green (`cargo fmt
--all -- --check`, `cargo test --workspace --locked`, `cargo test -p
mmd-engine --test separation` (32 tests, including
`a_bodied_scenario_is_pinned_to_a_golden_digest`,
`separation_is_capped_at_eight_neighbours` and
`separation_of_a_pair_is_equal_and_opposite` unedited), `cargo test -p
mmd-engine --test frame_allocations`, `cargo clippy --workspace
--all-targets --all-features -- -D warnings`, `nix flake check`, and the gate
smoke reproducing the T0-pinned digest
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
byte for byte). What is left needs eyes on a real window.

- [ ] `cargo run -- run --agents 5000` — a window opens and the horde moves
      exactly as it did before this ticket: same 48 px sprites, same
      edge-to-edge contact at chokepoints, same separation behaviour at
      chokepoints and in the corner-pocket case. Press `Esc` to quit and
      confirm a clean exit.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` —
      starts, the stacked spawn column still visibly spreads apart over the
      run (amortised over 4 phases, as in T3), no agent stuck inside the
      scene's obstacle, quits cleanly on `Esc`; indistinguishable from T3.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      unaffected by this ticket (stays at `separation_phases: 1`); same check
      as T3 at the lower-density scale.

## T5 push-priority-mass

Repulsion is now weighted by a per-agent push priority (`mass: Vec<u8>` /
`inv_mass: Vec<f32>` on `Simulation`, `mass[i] = (i % classes) + 1`): a
heavier neighbour pushes harder and is itself pushed less, so a dense goal
sink breaks its own symmetry instead of deadlocking. Every scene still at
`mass_class_count: 1` (`collision_mid_v1`, `technical_prototype_v1`, and the
`fixture_*` scenes) is bit-identical to before this ticket — a human should
observe **zero visible change** there. `collision_sprite_v1` alone now opts
into `mass_class_count: 2`. Everything automatable is green (`cargo fmt --all
-- --check`, `cargo test --workspace --locked`, `cargo test -p mmd-engine
--test separation` (36 tests, including `a_bodied_scenario_is_pinned_to_a_
golden_digest` and `a_bodyless_scenario_walks_the_flow_only_path` unedited,
plus the four new mass tests), `cargo test -p mmd-engine --test
frame_allocations`, `cargo clippy --workspace --all-targets --all-features --
-D warnings`, `cargo build -p mmd-engine --no-default-features --features
gpu`, `nix flake check`, all three xtask `--check` commands, and the gate
smoke reproducing the T0-pinned digest
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
byte for byte). What is left needs eyes on a real window.

- [ ] `cargo run -- run --agents 5000` — a window opens and the horde moves
      exactly as it did before this ticket (this scene stays at
      `mass_class_count: 1`): same 48 px sprites, same edge-to-edge contact at
      chokepoints. Press `Esc` to quit and confirm a clean exit.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` —
      unaffected by this ticket (stays at `mass_class_count: 1`); starts,
      spreads, no agent stuck inside the scene's obstacle, quits cleanly on
      `Esc`; indistinguishable from T4.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      this is the scene the ticket changed: it now runs two push-priority
      classes. Confirm the stacked spawn column still visibly opens up over
      the run, and that no agent ends up shoved inside a wall or obstacle.
      Quit cleanly on `Esc`.
