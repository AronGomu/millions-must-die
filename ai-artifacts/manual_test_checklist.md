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
