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
- [ ] Queue and produce a Worker and a Soldier from the HQ/Barracks and confirm each newly spawned unit appears standing just outside the producing building's footprint, not overlapping it (since T6 a unit is never placed on top of another at all: production picks the nearest free spot, and waits with the unit paid for and queued if there is none).
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

## T6 body-safe-production-and-construction

- [ ] Launch `cargo run -- rts`, park several workers right outside the HQ's exit side, then queue a Worker: confirm the new unit appears on a free spot *near* that exit rather than on top of anybody, and that the standing workers are not shoved to make room for it.
- [ ] Wall the HQ's exit in with idle units (queue and rally several units there, or walk the whole base into that corner) and queue one more Worker: confirm the production bar fills to 100% and then **waits** — no unit appears, no crystal is refunded, and the supply reading does not change while it waits.
- [ ] Clear one body's worth of space near that blocked HQ: confirm the waiting unit appears on the next tick, exactly once, and the queue entry disappears (crystal is not charged a second time).
- [ ] Queue two units back to back at one building with a crowded exit: confirm they come out one after the other, each on its own free spot, and none of them is ever drawn overlapping another.
- [ ] Place a Depot so its footprint covers two or three of your own idle workers and let it finish: confirm every covered worker is moved clear on the completion tick, all to different spots, none of them left standing inside the finished building.
- [ ] Watch the same completion closely: confirm the workers move *at the moment* the building becomes solid — not a beat later, and never visibly standing inside a finished building for a frame.
- [ ] Box a worker into a dead-end pocket that a Depot's own footprint would fill entirely, and build that Depot with that worker: confirm the build bar reaches the end and **holds** just short of finishing (the site stays walkable, the supply cap does not rise) instead of finishing on top of the worker.
- [ ] Walk that trapped worker out of the pocket (or cancel the site): confirm the site finishes normally on the following tick once its bodies can be moved clear.
- [ ] Finish two buildings on nearly the same tick with workers standing between them: confirm no worker is left inside either finished footprint, and no two workers end up merged.
- [ ] Run the tracked acceptance script (`cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`) and confirm the whole gather → build Depot → produce Worker → build Barracks → produce Soldier flow still completes visibly, with the produced units appearing clear of every other body.

## T7 persistent-settings

- [ ] Launch `cargo run -- rts`, quit immediately, then inspect `${SDL pref path}/AronGomu/MillionsMustDie/settings-v1.json` (Linux: `~/.local/share/AronGomu/MillionsMustDie/settings-v1.json`): confirm nothing was created there yet — this slice only loads settings, T13 is the first slice that saves from a real edit.
- [ ] Hand-write a `settings-v1.json` at that path with legal non-default values (e.g. `camera.keyboard_pan: 24`, `audio.master: 65`, `display.mode: "windowed1280x720"`) and launch `cargo run -- rts`: confirm the startup `rts: settings mode=... keyboard_pan=24 ... master=65 ...` line matches exactly what you wrote, with no `rts: settings warning=` line.
- [ ] Corrupt that file (e.g. truncate it to `{`) and relaunch: confirm the run still starts cleanly, prints an `rts: settings warning=` line naming the file path, and the settings debug line falls back to the documented defaults (`mode=BorderlessDesktop confine_pointer=true keyboard_pan=48 edge_pan=48 pause_on_focus_loss=false master=80 music=35 voice=70 sfx=60`) rather than crashing or silently keeping stale values.
- [ ] Set an out-of-range value by hand (e.g. `audio.master: 101`, or `camera.keyboard_pan: 50` which is not a multiple of 6) and relaunch: confirm the same warn-and-default behavior as the corrupt-file case.
- [ ] Run `SDL_VIDEODRIVER=offscreen cargo run -- rts --frames 3` with a *malformed* settings file already on disk at the real pref path: confirm the offscreen run never prints an `rts: settings warning=` line and the file on disk is left untouched (byte-for-byte) — the offscreen path must never look at it at all.

## T8 aspect-fit-canvas

- [ ] Launch `cargo run -- rts` in windowed mode (or resize the claimed window on a host/compositor that honours it) to a non-16:9 size, e.g. 1280x1024 or an ultrawide 3440x1440: confirm the rendered scene keeps its correct aspect ratio inside a centred region — no stretch, no squash — and the leftover area (left/right or top/bottom bars) is a flat clear colour, not part of the scene.
- [ ] With that same non-16:9 window, click in one of the clear-colour bars: confirm nothing happens — no selection, no order, no placement — while a click on the actual scene content still works normally.
- [ ] Move the mouse from inside the scene content out into a bar and hold it against the physical edge of the window: confirm the camera still edge-pans (the clamp-to-edge behavior), the same as parking the pointer against the content edge on an exact-16:9 window.
- [ ] Run a scripted session (`cargo run -- rts --frames 5 --inject-input "1:move:960,540;2:lclick:960,540"`) and confirm clicks/moves still land on the same logical cell they always have — scripted coordinates are unaffected by the live aspect-fit transform.

## T9 camera-frontier-and-speeds

- [ ] Launch `cargo run -- rts` and hold the right arrow key until the camera stops moving: confirm it stops with the map's right edge still fully filling the screen (no dead space beyond the diamond), not at some arbitrary distance short of it.
- [ ] Repeat holding left, up and down: confirm the camera stops at each of the map's four edges the same way, and never shows blank space past any of them.
- [ ] Park the pointer against a screen edge (edge-pan) at each of the four edges in turn: confirm edge-pan clamps at the same frontier the keyboard pan does.
- [ ] Hold an arrow key *and* park the pointer on a different edge at the same time: confirm the camera moves faster (both speeds add) while both are held, and still stops cleanly at the frontier.
- [ ] With `camera.keyboard_pan` and `camera.edge_pan` set to different values in `settings-v1.json` (e.g. `keyboard_pan: 24`, `edge_pan: 96`), launch `cargo run -- rts`: confirm holding an arrow key visibly pans slower than parking the pointer on an edge.
- [ ] While the camera is panned away from the base, watch units near the *bottom* of the screen relative to ones near the *top*: confirm the depth/occlusion ordering (nearer sprites still draw in front of farther ones) stays correct after the pan — it must not look like the pre-pan frame's depth is still in effect.
- [ ] Run the tracked acceptance script (`cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`) and confirm the panned-camera milestone still visibly moves the view away from the base and the rest of the flow (select → gather → build → produce) completes normally.
