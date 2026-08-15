# Manual test checklist

## T1 rts-contracts

- [ ] Launch `cargo run -- rts` and confirm the tracked map still renders at the same 320×320 scale it did before this change (no visual resize).
- [ ] Select a worker and a soldier, issue a move order across open ground, and confirm both units now visibly move noticeably faster than before (workers ~30 cells/sec, soldiers ~24 cells/sec — roughly 3x the old pace).
- [ ] Confirm the worker still clearly outruns the soldier when both are sent to the same distant point together.
- [ ] Click-select a unit at its edge (near the current selection ring) and confirm picking still feels correct — the pick radius now reflects a 3-cell body instead of 6-cell. (Widened by `T2`: a unit is now hit by its full 48×48 sprite quad *or* that body circle, whichever contains the click.)
- [ ] Confirm no visual or behavioral regression in building placement, gather, or production flows during a few minutes of normal play.

## T2 picking-and-context-orders

- [ ] Launch `cargo run -- rts`, select a worker, and left-click each of the four corners of a resource node's sprite (not its centre) in turn — every corner click should select the node, not miss it.
- [ ] With a worker or group selected, right-click near a corner (not the centre) of a resource node and confirm Gather starts (the worker walks to the node and begins mining).
- [ ] Select a mixed group (some workers, one soldier) and right-click a resource node: confirm the workers start gathering and the soldier instead walks toward the node instead of being ignored.
- [ ] Select a mixed group and right-click a building under construction (a site): confirm only workers attend it; the soldier does not move toward it and is not given any order.
- [ ] Right-click a finished (non-site) building or empty ground with units selected: confirm the whole selection walks to the clicked point as before.
- [ ] Overlap a worker's *sprite* with the HQ's (park it against the HQ's front edge — since `T3`/`T4` a body can no longer stand inside a finished footprint) and click precisely where the two drawn sprites coincide: confirm the frontmost-by-depth entity wins, not "units always win". At equal depth the lower entity slot wins.
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

- [ ] On a machine that has never run this build: launch `cargo run -- rts`, quit immediately, then inspect `${SDL pref path}/AronGomu/MillionsMustDie/settings-v1.json` (Linux: `~/.local/share/AronGomu/MillionsMustDie/settings-v1.json`): confirm nothing was created there — loading alone never writes. (Once you have committed a settings edit in the `T13` panel the file exists from then on; that is the only thing that creates it.)
- [ ] Hand-write a `settings-v1.json` at that path with legal non-default values (e.g. `camera.keyboard_pan: 24`, `audio.master: 65`, `display.mode: "windowed1280x720"`) and launch `cargo run -- rts`: confirm the startup `rts: settings mode=... keyboard_pan=24 ... master=65 ...` line matches exactly what you wrote, with no `rts: settings warning=` line.
- [ ] Corrupt that file (e.g. truncate it to `{`) and relaunch: confirm the run still starts cleanly, prints an `rts: settings warning=` line naming the file path, and the settings debug line falls back to the documented defaults (`mode=BorderlessDesktop confine_pointer=true keyboard_pan=48 edge_pan=48 pause_on_focus_loss=false master=80 music=35 voice=70 sfx=60`) rather than crashing or silently keeping stale values.
- [ ] Set an out-of-range value by hand (e.g. `audio.master: 101`, or `camera.keyboard_pan: 50` which is not a multiple of 6) and relaunch: confirm the same warn-and-default behavior as the corrupt-file case.
- [ ] Run `SDL_VIDEODRIVER=offscreen cargo run -- rts --frames 3` with a *malformed* settings file already on disk at the real pref path: confirm the offscreen run never prints an `rts: settings warning=` line and the file on disk is left untouched (byte-for-byte) — the offscreen path must never look at it at all.
- [ ] Repeat the previous step with `SDL_VIDEODRIVER=dummy`: same result. `dummy` is display-less too, and is now treated as fully isolated — no settings lookup, no window, no pointer grab.
- [ ] On a machine that has never run this build, run `cargo run -- rts --frames 0`: confirm it refuses the flag *and* that `~/.local/share/AronGomu/MillionsMustDie/` was still not created. A rejected flag must cost nothing, and `SDL_GetPrefPath` creates its directory just by being called.

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

## T10 window-modes-and-focus

- [ ] Set `display.mode: "borderless_desktop"` in `settings-v1.json` and launch `cargo run -- rts`: confirm the window comes up borderless, filling the desktop resolution, no title bar/border.
- [ ] Set `display.mode: "exclusive1920x1080"` and relaunch: confirm the display switches to a real exclusive 1920x1080 fullscreen mode (a monitor at a different native resolution briefly blanks/resyncs), not a scaled/letterboxed borderless window.
- [ ] Set `display.mode: "windowed1280x720"` and relaunch: confirm a bordered, resizable 1280x720 window appears centred on the display, and dragging its edges actually resizes it.
- [ ] With `display.confine_pointer: true` and the window focused: confirm the OS pointer cannot leave the window (drag the mouse hard toward an edge — it stops at the window boundary instead of reaching a second monitor/the desktop).
- [ ] Alt-Tab away from the focused window: confirm the pointer is released immediately (it can now reach the rest of the desktop) and any held pan key/edge-pan/in-progress drag box stops dead — the camera does not keep drifting while unfocused.
- [ ] Alt-Tab back to the window: confirm the pointer is re-confined per the `confine_pointer` setting, and nothing from before the focus loss (old drag box, old held pan direction) resumes — panning/dragging only resumes from fresh input given after regaining focus.
- [ ] Set `gameplay.pause_on_focus_loss: true`, launch, then Alt-Tab away: confirm the sim visibly pauses (HUD/tick stops advancing) while unfocused, and resuming happens only from an explicit unpause once focus returns (T13 wires the actual paused-menu UI; this slice only stops the tick).
- [ ] With `pause_on_focus_loss: false` (default), Alt-Tab away and back a few times: confirm the sim keeps ticking the whole time and nothing about camera/selection state is corrupted by the focus churn.
- [ ] Resize the windowed-mode window (or move it to a display with a different scale factor, if available) while it is running: confirm the rendered scene's aspect-fit content rect and mouse-click accuracy (T8) stay correct immediately after the resize/display change — no stale letterboxing, no click landing on the wrong cell.
- [ ] Run `SDL_VIDEODRIVER=offscreen cargo run -- rts --frames 3`: confirm no window is created, no "rts: window ... claimed"/"rts: released window" lines print, and the run exits clean exactly as before this ticket.

## T11 starcraft-hud-layout

- [ ] Launch `cargo run -- rts` and confirm the bottom of the screen shows three distinct panels: a minimap frame on the left (`[16,856]..[400,1064]`), a selection card in the centre (`[424,856]..[1304,1064]`), and a 3x3 command grid on the right (`[1696,856]..[1904,1064]`) — with visible gaps between them, not one continuous strip.
- [ ] Confirm a small gear icon renders in the top-right corner of the top bar (`[1872,8]..[1904,40]`).
- [ ] Select a single worker: confirm a 128x128 portrait (cropped from the worker sheet) appears at the selection card's left edge, with kind/carry text to its right.
- [ ] Select a single HQ or Barracks: confirm its portrait shows the building art, "READY", and (if a rally point is set) a `RALLY x,y` line.
- [ ] Box-select more than 24 units: confirm the selection card switches to an 8x3 grid of 48px icons (first 24 by ascending id) and a `+N` marker appears to the right of the grid for the remainder.
- [ ] With only workers selected: confirm the command grid's top-left three cells light up with distinct build icons (HQ/Depot/Barracks).
- [ ] With a single finished HQ selected: confirm the command grid shows a Train-Worker icon top-left and a Rally icon bottom-right (slot 8), and nothing else.
- [ ] With a single finished Barracks selected: confirm the command grid shows a Train-Soldier icon top-left and the same Rally icon bottom-right.
- [ ] Select nothing, then select a worker and a building together: confirm the command grid goes fully blank in both cases.
- [ ] Run the tracked acceptance script (`cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`) and confirm the HUD renders throughout without visual corruption (no wrong-sheet sampling, no panel z-fighting with world sprites).

## T12 hud-routing-and-minimap

- [ ] Launch `cargo run -- rts` and confirm the minimap panel (`[32,872]..[384,1048]`) shows a filled diamond frame and a small bright outlined quad (the camera's own footprint) somewhere inside it — not entities, resources, fog, or terrain detail.
- [ ] Hold an arrow key to pan the camera: confirm the camera-polygon quad on the minimap moves and reshapes to track the live view, staying inside the map diamond.
- [ ] Click inside the minimap's map area, off-centre: confirm the main camera recentres on roughly that part of the map, clamped the same way keyboard/edge panning is (no dead space past the map edge).
- [ ] Click a point inside the minimap's pixel box but outside the actual diamond (e.g. one of its four corners): confirm nothing happens — no camera jump, no crash.
- [ ] Try a click-drag starting on the minimap: confirm it does not pan/select/drag anything — the minimap has no drag gesture.
- [ ] Select several units (more than one, drawn as the icon grid in the selection card): click one icon plainly — confirm the selection collapses to just that unit. Shift-click a still-selected icon — confirm it drops out of the selection while the rest stay selected.
- [ ] With a worker selected, click the BuildHq icon in the command grid (top-left cell): confirm a build ghost opens, identical to pressing `Q`.
- [ ] With nothing selected (command grid fully blank/disabled), click anywhere in the 3x3 command-grid area: confirm nothing happens — no ghost opens, no order issues.
- [ ] Select a finished HQ or Barracks, click the Rally icon (bottom-right, slot 8), then click on the gear icon in the top bar and Escape back out of the menu it opens (`T13` — the gear no longer no-ops, see the `T13` section below): confirm nothing about the rally arming state changes across that detour. Then click a valid point on the map: confirm the rally point is set there (visible as `RALLY x,y` in the selection card's detail text next time that building is selected).
- [ ] Click repeatedly inside the gaps of the bottom HUD panel (between the minimap/selection/command cards, and in the panel's own margins) and inside the top bar away from the gear icon: confirm none of these ever select a unit, issue a move/gather/build order, or otherwise change world state — every one of them is silently absorbed by the HUD.
- [ ] Run the tracked acceptance script (`cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`) and confirm it still completes cleanly (same milestones: select → gather → build → produce) with the new HUD-first pointer routing in place.

## T13 settings-menu

- [ ] Launch `cargo run -- rts`, press `Escape`: confirm a small one-button paused modal (`SETTINGS`) appears centred over the HUD/world, and the sim visibly pauses (units stop moving/gathering).
- [ ] Press `Escape` again: confirm the modal closes and the sim resumes (units move again), with no quit/exit.
- [ ] Click the gear icon (top-right of the top bar) instead of pressing Escape: confirm it opens the exact same paused menu.
- [ ] From the paused menu, click `SETTINGS`: confirm a larger settings panel opens (window mode buttons, keyboard/edge pan sliders, confine-pointer and pause-on-focus-loss checkboxes, four volume sliders, a `BACK` button).
- [ ] Click `BACK`: confirm it returns to the one-button paused menu (not straight to gameplay). Press `Escape` from there: confirm it now returns to gameplay.
- [ ] Press `Space` to pause manually, then open and fully close the menu (gear/Escape, then Escape again): confirm the game is still paused afterward (manual pause survives the menu). Press `Space` again to confirm it then resumes.
- [ ] Alt-tab away from the window with `pause_on_focus_loss` on (toggle it on in Settings first, then click `BACK`/`Escape` back to gameplay): confirm losing focus opens the paused menu automatically. Alt-tab back and close the menu: confirm the sim resumes.
- [ ] While a settings panel control is open, click far outside the panel (e.g. the middle of the world or the HUD's command grid): confirm nothing in the world or the HUD reacts — the click is fully absorbed by the modal.
- [ ] Click each window-mode button (`BORDERLESS`, `EXCLUSIVE`, `WINDOWED`) in turn: confirm the real window visibly changes mode each time, the newly-selected button highlights, and no crash/black window occurs on any transition. The commit now releases the GPU claim, transitions, reclaims and refreshes the viewport — so also confirm the run does **not** die with `present failed` right after a switch, and that clicking a HUD/world point immediately afterward still hits what you aimed at (a stale viewport would offset every click).
- [ ] Drag-click along the keyboard-pan and edge-pan tracks: confirm the displayed number snaps to a multiple of 6 between 6 and 96, and holding an arrow key afterward visibly changes camera pan speed to match.
- [ ] Click the confine-pointer and pause-on-focus-loss checkboxes: confirm each visibly toggles (tint/label state) and, for confine-pointer, the OS pointer is actually grabbed/released to match.
- [ ] Drag-click along each volume track (Master/Music/Voice/SFX): confirm the displayed number snaps to a multiple of 5 between 0 and 100 (no audible effect yet — `T16`).
- [ ] After changing several settings, quit and relaunch `cargo run -- rts`: confirm every changed value (window mode, pan speeds, confine/focus toggles, volumes) is exactly what was last set — the `rts: settings mode=... ...` startup line should match.
- [ ] Force a save failure (e.g. remove write permission on the settings directory, or point `SDL_VIDEODRIVER`'s pref dir at a read-only path) and change a setting: confirm a `SETTINGS NOT SAVED: <reason>` line appears in the settings panel and the control visibly reverts to its old value.
- [ ] Run the tracked acceptance script (`cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`) and confirm it still completes cleanly and quits at the same milestone (now via the `quit` script token instead of `Escape`).
- [ ] With a non-default `camera.keyboard_pan` (e.g. 24) persisted, run that same tracked script *windowed* (no `SDL_VIDEODRIVER`) and again with `SDL_VIDEODRIVER=offscreen`: confirm both exit lines carry the **same** `hash=` and the same `camera=`, and that your persisted `camera.keyboard_pan` is unchanged on disk afterward. A scripted run is a replay: it takes the default camera speeds and never writes settings.

## T14 generated-audio-assets

- [ ] Run `cargo run -p xtask -- audio` from a clean checkout: confirm it writes `music_placeholder.wav`, five `voice_*.wav` files, `ui_click.wav`, and `manifest.json` under `assets/audio/generated/`, with no network access and no `.wav`/binary present anywhere else in the repo.
- [ ] Run `cargo run -p xtask -- audio` twice into two separate directories (or regenerate and diff against the tracked copy): confirm every generated file is byte-identical both times.
- [ ] Run `cargo run -p xtask -- audio --check`: confirm it prints `audio: ok (7 wav + manifest)` and exits 0 against the tracked assets with no edits.
- [ ] Open `music_placeholder.wav` in any PCM-capable player/tool (e.g. `ffprobe`): confirm 48,000 Hz, 16-bit, stereo, ~8s duration, and that it loops without an audible click (first/last sample are silence by construction).
- [ ] Play each `voice_*.wav`/`ui_click.wav`: confirm each is a short (60-180ms) clean tone with an audible fade-in/out, not a click/pop, and that no two cues sound identical.
- [ ] Read `assets/audio/README.md`: confirm it documents the generated provenance, the WAV replacement contract (source/license/attribution/checksum), and contains no StarCraft/Blizzard/Terran audio file or URL — only the negative statement disclaiming them.

## T15 audio-events-and-buses

- [x] Superseded by `T16`: interactive runs now play through a real SDL audio device (`SdlAudioSink`). See the `T16 sdl-audio-runtime` section below for the audible checks; offscreen/test runs still use the fixed fake sink and never open a device.
- [ ] Launch `cargo run -- rts`, play for a few seconds (select units, right-click to move/gather, click a command card, click the minimap, open the gear menu), quit, and read the `rts: audio ...` line printed just above `rts: clean exit`: confirm `music=1`, that `voice`/`cues` grew with the selections and orders you actually made, that `ui` grew by exactly one per pointer click on the gear/menu/settings control/command card/valid minimap point, and `gains=2800/5600/4800` for default volumes.
- [ ] Repeat a selection you already have selected (drag the same box twice, or click a unit that is already the whole selection): confirm `cues` does not grow the second time — only newly selected units voice.
- [ ] Select more than eight units and give them one move order: confirm `cues` grows by at most 8 for that order and `reject` stays at 0 — the cap is not a rejection.
- [ ] Right-click somewhere no selected unit can take the order (e.g. a build site with only soldiers selected, or a target with no formation space): confirm `reject` grows by exactly one for that click, not one per unit.
- [ ] Press a build/produce hotkey (`Q`/`W`/`E`/`A`/`S`) instead of clicking its card: confirm `ui` does not grow — keyboard actions never make a pointer-click sound.
- [ ] Open Settings, drag a volume slider, then quit and relaunch: confirm the new `gains=` values on the `rts: audio` line match `master% * bus%` for the values shown in the settings panel (e.g. master 50 / music 35 → `1750`).
- [ ] Run the tracked acceptance script (`cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`) twice: confirm both runs print an identical `rts: audio` line and an identical `hash=` on the exit line.

## T16 sdl-audio-runtime

- [ ] Launch `cargo run -- rts` on real hardware with a real audio device (no `SDL_AUDIODRIVER` override): confirm placeholder music starts immediately and loops continuously with no audible click at the loop boundary.
- [ ] Alt-tab away from the window (with or without `pause_on_focus_loss` on) and back: confirm the music keeps playing the whole time, including while the T13 paused menu is open.
- [ ] Select a unit, then give it a move/gather/build order: confirm you hear a short, distinct voice cue for the selection and a different distinct cue for the order (select/move/gather/build all sound different).
- [ ] Select up to eight units and give them a mixed batch of orders in one action: confirm you hear up to eight overlapping cues, and that a second action's cues fully replace the first's (no leftover cue from the previous action lingering).
- [ ] Right-click an invalid target (e.g. a build site with only soldiers selected): confirm a distinct reject sound plays, and it never cuts off or is cut off by a simultaneous accepted unit cue.
- [ ] Click the gear icon, a settings control, a command-grid card, and a valid minimap point in quick succession (5+ clicks within about a second): confirm each produces a short UI click sound, and the fifth-and-later clicks still produce a click (the 4-lane round robin steals the oldest slot rather than going silent).
- [ ] Open Settings and drag the Master/Music/Voice/SFX sliders one at a time while music/voice/UI sounds are playing: confirm each slider's bus changes volume live and independently (e.g. Music to 0 silences only the music, not the voice cues).
- [ ] With `SDL_AUDIODRIVER` set to a nonexistent driver name and a real (non-offscreen) window, run `cargo run -- rts`: confirm the process exits with a nonzero code and an actionable message naming the audio failure, rather than falling back to silent/offscreen play.
- [ ] Confirm `SDL_VIDEODRIVER=offscreen SDL_AUDIODRIVER=invalid cargo run -- rts --frames 3` still exits 0 and prints an `rts: audio` line — an offscreen run must never depend on (or be broken by) a real audio device.

## T17 phase1-1-acceptance

The offscreen run proves state machines, assets and events. It cannot prove
sound or display hardware, and no agent may open a window, grab the pointer
or play audio on a live desktop — so everything below is human-only.

- [ ] Run the merge-gate command in its real windowed form on this desktop: `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`. Confirm it exits 0 and its `rts: clean exit` line ends with `body_overlaps=0 ui_page=gameplay music_starts=1 voice_select=8 voice_order=9 voice_reject=1 sfx_ui=8 keyboard_pan=78`.
- [ ] Watch that same windowed run: confirm you *see* the six workers box-selected, walking to the crystal and gas nodes, the Depot and Barracks ghost-then-build, and a Worker and a Soldier pop out — the run is not just a green exit line.
- [ ] Watch the last 300 frames of it: confirm the camera jumps to the map's far right when the script clicks the minimap, the paused menu and then the settings panel open, the keyboard-pan slider visibly lands on 78, both Escapes back all the way out, and the units start moving again once the menu closes.
- [ ] Listen to that run on real hardware: confirm you hear music, eight selection cues, nine order cues, exactly one reject when the script orders off the map, and eight UI clicks — the counters above are derived, not recorded from the speakers.
- [ ] Open the settings panel by hand during that run's menu section is not possible (it is scripted); instead launch `cargo run -- rts`, open Settings, click the keyboard-pan track at the same place the script does, and confirm the panel shows `78` and holding an arrow key afterward pans visibly faster.
- [ ] Confirm the scripted settings edit did **not** persist: after the acceptance run, relaunch `cargo run -- rts` and check the `rts: settings ...` startup line still shows your own `keyboard_pan`, not 78. A scripted click commits in memory only.
- [ ] Play for a few minutes and try to force two units into each other (order a large group through a one-unit-wide gap, park units on a build site as it completes, push a group into a corner): confirm no two unit sprites ever visually interpenetrate, matching the `body_overlaps=0` the exit line reports.
- [ ] Window mode, pointer confinement and Alt-Tab behaviour are still window/OS evidence only (see the `T10` and `T13` sections) — the offscreen acceptance run claims nothing about them.

## T18 docs-and-phase-close

Docs-only slice: no game behaviour changed. The offscreen gate proved every
command below except what a browser or a human eye must judge.

- [ ] Open `docs/rts-interaction-ui-audio-hardening-architecture.html` in a browser: confirm the badge reads `IMPLEMENTED · CLOSED ON FUNCTIONAL SCOPE`, every SVG diagram (system map, collision tick, HUD regions, audio pipeline) still renders and is legible at desktop width, the new "Where it lives" and "As built" tables lay out without overflow, and the worker sprite thumbnail loads.
- [ ] From that page, click each link in the header and footer (functional close, phase 1 architecture, horde collision, gate, ADR 016–020): confirm all open the intended file.
- [ ] Open `docs/rts-engine-prototype-architecture.html`: confirm the new "Superseded in part by phase 1.1" paragraph reads as a forward pointer, not as a rewrite of what phase 1 claimed, and the footer's "Next:" link works.
- [ ] Open `docs/agent-collision-architecture.html`: confirm the new scope paragraph makes it unmistakable that the page is horde-only and that nothing on it claims hard collision for the horde.
- [ ] Read `docs/rts-interaction-ui-audio-hardening-functional-close.md` end to end and confirm nothing in it overstates what you have actually seen on this machine — in particular that no claim of audible sound, of a real window, or of a pointer grab appears outside the "proof boundaries" table.
- [ ] Run the merge gate's RTS smoke in its real windowed form — `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` — and confirm it exits 0 with the exit line ending `body_overlaps=0 ui_page=gameplay music_starts=1 voice_select=8 voice_order=9 voice_reject=1 sfx_ui=8 keyboard_pan=78`. The agent ran this offscreen only; the windowed run is the one that proves window + pointer + audio.
- [ ] Run `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` and the `collision_sprite_v1` twin in their real windowed form and confirm both exit cleanly; the agent ran them offscreen.
- [ ] Confirm `cargo run -p xtask -- audio --check` prints `audio: ok (7 wav + manifest)` on your machine, and that `git status` is clean afterwards.

## T1 interaction-fsm-menu-close

- [ ] Launch `cargo run -- rts`: confirm top-right control shows framed text `MENU` (not a gear icon); click it and confirm the pause menu opens with a Menu SFX.
- [ ] Hover and press `MENU`, command-grid cells (empty + filled), selection icons, Settings/Close/Back/window-mode/checkbox rows: confirm idle/hover/pressed/selected/disabled frames are pairwise-distinct and readable.
- [ ] Open pause menu: confirm Settings still at centre `[960,540]` and Close Menu sits directly below; click Close Menu and confirm return to gameplay with one Menu SFX.
- [ ] Space to pause, open Menu, Close Menu: confirm manual Space pause is still active (sim stays paused) after close.
- [ ] Open Menu, press on Settings, drag to Close Menu, release: confirm neither Settings nor Close activates.
- [ ] Open Menu, press on modal chrome, drag onto the world, release: confirm no world select/order.
- [ ] Escape nesting still works (Gameplay→Menu→Settings→Menu→Gameplay); no F10 binding appears.
- [ ] Open Settings: confirm panel is wider, audio rows sit lower (96px pitch), checkbox label rows are fully hittable, Back still works; keyboard-pan track at x=1170 still reads 78 after a click there.

## T2 settings-schema-grid-mutes

- [ ] Back up any existing `~/.local/share/AronGomu/MillionsMustDie/settings-v1.json`, replace it with a pre-grid legacy body (schema 1, no `show_grid` / `*_muted` keys, non-default pan/volumes), launch `cargo run -- rts`, quit: confirm startup `rts: settings ...` line keeps the old numeric values and ends with `show_grid=true master_muted=false music_muted=false voice_muted=false sfx_muted=false` and no warning.
- [ ] After that first load, open Settings (once UI exposes mutes) or hand-edit the saved file: confirm a subsequent save keeps `schema_version: 1`, same filename, and writes explicit `show_grid` + four mute keys; second identical save is byte-stable.
- [ ] Hand-write `"music_muted": "yes"` into the JSON and relaunch: confirm warn-and-default (defaults load, warning names the file) rather than a crash or partial mute state.
- [ ] With music playing, force mute via settings JSON (`master_muted: true`) and relaunch interactive: confirm levels in the debug line stay at prior numbers while audio is silent; flip flag back to false and confirm prior loudness returns without restarting streams mid-cue feel.
- [ ] `SDL_VIDEODRIVER=offscreen cargo run -- rts --frames 3` still prints no settings warning and never touches the real pref path (defaults only).

## T3 live-sliders

- [ ] Launch `cargo run -- rts`, open Settings: confirm each of the six numeric rows (Keyboard Pan, Edge Pan, Master, Music, Voice, SFX) shows a track, filled range, light thumb, and a framed value field to the right.
- [ ] Drag Keyboard Pan from the default (48) toward 60 and release outside the track: confirm the value and camera pan speed update *while dragging* (not only on release), ownership stays on that slider, and only one Settings SFX plays for the whole drag.
- [ ] Drag Master (or Music/Voice/SFX) across several steps: confirm audio level changes live during the drag and repeated motion inside one step does not spam clicks/saves.
- [ ] Click (no drag) a point on a volume track: confirm it still snaps to the nearest legal step (0..100 step 5) exactly as before.
- [ ] Force a save failure mid-drag if you can (e.g. replace the settings file with a directory while the panel is open, then drag): confirm the old value/runtime stay, a `SETTINGS NOT SAVED:` warning appears, and a later legal drag step still works without releasing first.
- [ ] While dragging a slider, release over the world: confirm no unit selection/order and the settings page stays open.

## T4 numeric-editing

- [ ] Launch `cargo run -- rts`, open Settings, click a framed value field (right of Keyboard Pan): confirm it shows selected frame and the current digits; type `999` and press Enter: confirm value becomes 96 (pan max) and camera pan speed matches; one Settings SFX on commit.
- [ ] Click Master field, type `53`, Enter: confirm it snaps to 55; drag is unnecessary — typed path uses the same clamp_snap.
- [ ] Click a field, Backspace until empty, Enter: confirm old value remains and no save/gain change occurs.
- [ ] Click a field, type a new value, press Escape: confirm original value restored and page stays Settings (one Escape does not back to pause menu).
- [ ] With a valid buffer in a field, click the PAUSE ON FOCUS LOSS checkbox: confirm the typed value commits first, then the checkbox toggles.
- [ ] With a valid buffer in a field, Alt-Tab away (focus loss): confirm value is saved, text input stops (no stuck IME), pointer/keys clear, and if pause-on-focus-loss is on the pause menu opens.
- [ ] With no field focused, Escape nesting still works: Gameplay→Menu→Settings→Menu→Gameplay.

## T5 mute-label-controls

- [ ] Launch `cargo run -- rts`, open Settings: confirm each of the four audio rows (Master, Music, Voice, SFX) shows a framed label button above its slider (text: "MASTER", "MUSIC", "VOICE", "SFX" in idle/green tint when unmuted).
- [ ] Click the MASTER label: confirm it turns selected (green frame), text changes to "MASTER MUTED", all audio buses go silent, and the master/per-bus numeric sliders and fields remain interactive showing the same stored numbers.
- [ ] Click MASTER label again: confirm it reverts to idle, text returns to "MASTER", audio restores to the exact previous levels, and the slider/field values are unchanged.
- [ ] Click MUSIC label to mute it, then drag the MUSIC slider to a new value: confirm the stored level changes (slider and field update), but the effective music gain stays 0 while muted.
- [ ] Unmute MUSIC: confirm the new stored level is immediately heard (gain goes to the dragged value, not the old pre-mute value).
- [ ] Click VOICE label to mute, then click it again (unmute): confirm only the voice bus is affected; master, music, SFX are unaffected.
- [ ] Open Settings, mute SFX, close Settings, relaunch `cargo run -- rts` with the same pref path: confirm sfx_muted persists in the settings file and SFX remains muted on relaunch.
- [ ] Force a save failure (replace settings file with a directory) and click a mute label: confirm the flag rolls back (label stays idle), the old gains are restored, and a `SETTINGS NOT SAVED:` warning appears.

## T6 scroll-and-clip-settings-body

- [ ] Launch `cargo run -- rts`, open Settings: confirm the scrollbar track (right edge) and thumb are visible; thumb top is flush with the body viewport top at scroll=0.
- [ ] Scroll mouse wheel down inside the settings body: confirm body content moves up (SFX rows come into view), thumb moves down; scroll back up to confirm return to offset=0 and thumb top returns to viewport top.
- [ ] Wheel while pointer is in the scrollbar area (not the body): confirm the scroll still works (consumed silently, no world action).
- [ ] Wheel on any other page (gameplay, pause menu): confirm no scroll change occurs.
- [ ] Drag the scrollbar thumb from top to bottom: confirm it reaches exactly max offset (128 px) and no further; drag from bottom to top: confirm it returns to 0.
- [ ] Click the scrollbar track above the thumb: confirm one-page-up scroll; click below: one-page-down.
- [ ] At max scroll, confirm the Back button and any warning text are still visible and clickable (fixed footer, unaffected by scroll).
- [ ] Scroll part-way, confirm the SFX slider and mute label respond to clicks at their displayed (scrolled) positions; confirm clicking at their content position (off-screen above) is consumed.
- [ ] Confirm rendered body panels do not bleed outside the body viewport (no pixel spill above y=160 or below y=880).
- [ ] Close Settings (Back), reopen: confirm scroll resets to 0 (or persists, depending on product decision — the exit line will show `settings_scroll_px=<rounded>` for verification).

## T7 — Positional command keys (QWE/ASD/ZXC)

- [ ] Q on selected Worker opens HQ ghost; W opens Depot ghost; E opens Barracks ghost
- [ ] Q on selected Barracks queues Soldier; C arms rally
- [ ] X on selected Worker with slot 7 empty/disabled: no cancel, no SFX
- [ ] Right-click while ghost pending: ghost cancels, no build order placed
- [ ] Hotkey letters Q/W/E/A/S/D/Z/X/C visible in bottom-right of each command cell
- [ ] Disabled/empty cells still show their hotkey letter
- [ ] R key: no effect in any context (unbound)
- [ ] Banner text visible in OS window title: "QWE/ASD/ZXC card, right-click cancel"
- [ ] Pointer clicks on command card still work as before (shared execute_slot path)

## T9 — building sprite pick and full stats card

- [x] `cargo fmt --all -- --check` exits 0
- [x] `cargo check --workspace --all-targets --all-features --locked` exits 0
- [x] `cargo test -p mmd-engine --test rts_selection --locked` exits 0 (41 tests)
- [x] `cargo test -p mmd-engine --test rts_hud --locked` exits 0 (67 tests)
- [x] `cargo test -p mmd-engine --test frame_allocations --locked -- --test-threads=1` exits 0 (26 tests)
- [ ] Manual: click on the top/side of a building sprite (above the footprint) — confirm it selects the building
- [ ] Manual: click on a footprint cell below the building sprite — confirm it selects the building
- [ ] Manual: select HQ with no queue — confirm six-line card: HQ / READY / SUPPLY +10 / QUEUE - / PROGRESS - / RALLY -
- [ ] Manual: enqueue Workers at HQ — confirm QUEUE W,W,... and PROGRESS N% update live
- [ ] Manual: rally flag set — confirm RALLY x,y on line 6
- [ ] Manual: select Barracks — confirm SUPPLY +0 on line 3
- [ ] Manual: select an under-construction site — confirm BUILDING N% on line 2, remaining lines explicit
