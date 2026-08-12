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
- [ ] Queue and produce a Worker and a Soldier from the HQ/Barracks and confirm each newly spawned unit appears standing just outside the producing building's footprint, not overlapping it (since T4, a unit that would appear on top of another is moved clear on the next tick rather than left merged).
- [ ] Run the full tracked acceptance script (`cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`) in a window and confirm it still completes the whole select → gather (crystal and gas) → build Depot → produce Worker → build Barracks → produce Soldier → pan-camera flow visibly, ending in a clean quit.

## T4 hard-unit-collision

- [ ] Launch `cargo run -- rts`, select all six starting workers and send them to one distant point: confirm they arrive spread across their own formation slots (since T5) with visible gaps and that **no two unit sprites ever overlap or pass through each other** at any point of the walk.
- [ ] Send two workers at one shared point midway between them (since T5, right-clicking a worker's *own* cell snaps the order to a free slot beside it, so aiming them at each other no longer produces a head-on walk): confirm they meet and stop a body's width apart instead of merging, and neither one is left jittering on the spot.
- [ ] Send one worker straight through a standing (idle) worker: confirm the standing worker is visibly *shoved aside* along the line between them and the mover continues, rather than the mover walking through it or freezing in front of it.
- [ ] Line three idle workers up in a row and walk a fourth into the back of the line: confirm the whole row shuffles forward together, and that nobody is pushed into a building, a resource node or terrain.
- [ ] Park a worker tight against a building's edge, then send another worker past it on that side: confirm the passing worker steers around it instead of stopping dead (it may take a slightly curved path).
- [ ] Send five workers to gather from one crystal node: confirm they take turns at the node — some mine while the rest wait clear of them — and that crystal keeps being delivered rather than the group deadlocking.
- [ ] Produce several units in a row from the HQ with workers standing right outside it: confirm each new unit appears clear of the others and the crowd sorts itself out instead of stacking on one spot.
- [ ] Run the tracked acceptance script (`cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`) and confirm the whole gather → build Depot → produce Worker → build Barracks → produce Soldier flow still completes visibly, with no unit ever drawn on top of another.
- [ ] Watch a busy base for a minute or two and confirm no unit is ever left permanently stuck against another unit with its order still active.

## T5 formations-and-fair-chokes

- [ ] Launch `cargo run -- rts`, box-select the six starting workers and right-click a distant patch of open ground: confirm they spread into a grid of evenly spaced final positions (about one body-width apart) instead of piling onto the clicked point, and that every one of them ends up standing still with no order left running.
- [ ] Repeat the same order twice in a row from different starting scatterings: confirm the group always lands on the same shaped block around the clicked point (deterministic slots), with only who-stands-where differing.
- [ ] Right-click a spot right up against a wall or inside a building's footprint: confirm the group still accepts the order and forms up on the nearest legal ground beside it, rather than the click being silently ignored.
- [ ] Box-select the six workers and right-click a crystal node: confirm each worker walks to its *own* spot around the node, they all mine and deliver, and none of them stands on the node's tile or on another worker.
- [ ] Select a mixed group (workers plus a soldier) and right-click a resource node: confirm the workers gather while the soldier takes its own spot around the same node instead of shoving into the mining ring.
- [ ] Select several workers and right-click a building under construction: confirm each one walks to a distinct spot around the site and attends it (the build bar moves faster with more of them), and that a soldier in the same selection is given no order at all.
- [ ] Send a group into a narrow gap (a one-body-wide choke between terrain): confirm they file through one at a time, each one taking its turn, and that nobody is left permanently waiting while another repeatedly cuts in front.
- [ ] Send a large group (a dozen or more, produced from the HQ) to one point: confirm the formation grows outward in rings and every member still finishes its walk rather than the outer ones grinding against the inner ones.
- [ ] Order a group into a fully enclosed pocket that only one unit could stand in: confirm nothing moves at all (the whole order is refused) rather than one unit walking off alone.
- [ ] Run the tracked acceptance script (`cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`) and confirm the whole select → gather → build → produce flow still completes visibly, with the gathering workers visibly spread around their node.
