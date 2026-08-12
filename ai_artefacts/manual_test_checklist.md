# Manual test checklist

## T1 rts-contracts

- [ ] Launch `cargo run -- rts` and confirm the tracked map still renders at the same 320×320 scale it did before this change (no visual resize).
- [ ] Select a worker and a soldier, issue a move order across open ground, and confirm both units now visibly move noticeably faster than before (workers ~30 cells/sec, soldiers ~24 cells/sec — roughly 3x the old pace).
- [ ] Confirm the worker still clearly outruns the soldier when both are sent to the same distant point together.
- [ ] Click-select a unit at its edge (near the current selection ring) and confirm picking still feels correct — the pick radius now reflects a 3-cell body instead of 6-cell.
- [ ] Confirm no visual or behavioral regression in building placement, gather, or production flows during a few minutes of normal play.

## T2 picking-and-context-orders

- [ ] Launch `cargo run -- rts`, select a worker, and left-click each of the four corners of a resource node's sprite (not its centre) in turn — every corner click should select the node, not miss it.
- [ ] With a worker or group selected, right-click near a corner (not the centre) of a resource node and confirm Gather starts (the worker walks to the node and begins mining).
- [ ] Select a mixed group (some workers, one soldier) and right-click a resource node: confirm the workers start gathering and the soldier instead walks toward the node instead of being ignored.
- [ ] Select a mixed group and right-click a building under construction (a site): confirm only workers attend it; the soldier does not move toward it and is not given any order.
- [ ] Right-click a finished (non-site) building or empty ground with units selected: confirm the whole selection walks to the clicked point as before.
- [ ] Stand a worker exactly on top of the HQ (drag it there or park it) and click precisely on the shared spot: confirm the click now selects the HQ, not the worker, when their sprites are drawn at the same depth (this is an intentional behavior change from the old "units always win" rule).
- [ ] Click a worker that is standing slightly in front of (further down-screen than) another overlapping unit or the HQ: confirm the frontmost (visually on-top) unit is the one selected.
- [ ] Right-click while a build-placement ghost is showing: confirm it still cancels the ghost instead of issuing a move/gather/build order.

## T3 radius-aware-static-navigation

- [ ] Launch `cargo run -- rts` on the tracked scene and confirm the six starting workers no longer overlap on spawn — they should appear spread across a wider area around the HQ instead of stacked in a tight one-cell-apart row.
- [ ] Select all six starting workers (drag-box around the base) and confirm every one of them is picked, even though they no longer sit in a tight row.
- [ ] Right-click a resource node with a worker selected and watch the whole round trip: confirm the worker visibly stops walking *before* reaching the node's own tile (it mines from just outside the node, never standing on top of it) and the same holds on the return leg at the HQ.
- [ ] Order a worker to walk close past a cluster of terrain obstacles and confirm it visibly keeps a small clearance gap from each obstacle's edge, rather than grazing or clipping through the corner of one.
- [ ] Place a Depot or Barracks next to existing terrain and confirm the ghost still only rejects placement for genuinely overlapping/blocked footprints — a legal, terrain-clear spot right up against an obstacle should still be placeable (placement validity is intentionally *not* widened by the new body-clearance rule).
- [ ] Queue and produce a Worker and a Soldier from the HQ/Barracks and confirm each newly spawned unit appears standing just outside the producing building's footprint, not overlapping it or another unit.
- [ ] Run the full tracked acceptance script (`cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`) in a window and confirm it still completes the whole select → gather (crystal and gas) → build Depot → produce Worker → build Barracks → produce Soldier → pan-camera flow visibly, ending in a clean quit.
