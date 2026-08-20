# Grill: RTS feedback polish

## Facts (scout)

- Current modal buttons render panel rects, but no hover/pressed states. HUD menu is gear icon only — source: `crates/mmd-engine/src/rts/hud.rs:26-30,873-898,959-1064`
- Pause menu has only `Settings`; Escape alone reaches private close behavior — source: `crates/mmd-engine/src/rts/hud.rs:726-802`, `src/rts_ui.rs:166-201`
- Six numeric settings already have click-to-snap tracks + live runtime commits. Numbers are text, not inputs; slider drag absent — source: `crates/mmd-engine/src/rts/hud.rs:73-113,779-931`, `src/rts_run.rs:902-970`
- Audio labels are not clickable. Volume `0` mutes; no mute flags or prior-level restore exist — source: `src/rts_settings.rs:75-93`, `src/rts_ui.rs:79-108`
- Settings panel has no scroll offset, clipping viewport, scrollbar, or `Event::MouseWheel` route — source: `crates/mmd-engine/src/rts/hud.rs:48-87,758-871`, `src/rts_run.rs:830-1010`
- F10 is unbound. Escape performs nested navigation — source: `src/rts_input.rs:48-68`, `src/rts_ui.rs:166-175`
- World grid render + persisted grid setting do not exist — source: `crates/mmd-engine/src/rts/pack.rs:281-470`, `src/rts_settings.rs:95-199`
- Placement validates exact cursor-centered footprint. Units never block placement. No nearby valid-candidate search exists — source: `crates/mmd-engine/src/rts/build.rs:99-187`, `src/rts_run.rs:346-354`
- Player buildings are already footprint-clickable. Detail card already shows kind, READY/build %, rally; rendered upper sprite is not part of building pick shape — source: `crates/mmd-engine/src/rts/selection.rs:245-327`, `crates/mmd-engine/src/rts/hud.rs:470-614`
- Finished Barracks already exposes Soldier in commandcard slot 0; click uses shared executor; current key is `S`, not slot-positional `Q` — source: `crates/mmd-engine/src/rts/hud.rs:267-337`, `src/rts_ui.rs:224-299`, `src/rts_input.rs:48-68`
- Area selection already draws green, border-only, four 2px quads — source: `crates/mmd-engine/src/rts/pack.rs:133-135,442-470`
- Gather phases: `ToNode`, `Mining`, `Returning`. Current collision checks every unit pair regardless kind/order. Any gathering exemption requires ADR 017 amendment — source: `crates/mmd-engine/src/rts/orders.rs:58-97`, `crates/mmd-engine/src/rts/world.rs:1996-2389`, `docs/ADR/017_ADR_rts_hard_collision_navigation_and_formations.md`

## Round 1 — UX and gameplay contracts

| # | Question | Answer | Precision |
| --- | --- | --- | --- |
| 1 | Interactive visual contract | Framed controls: idle, hover, pressed, selected, disabled | — |
| 2 | Close Menu pause behavior | Clear menu/focus pause only; preserve manual pause | — |
| 3 | F10 behavior | Pending | Asked whether Escape already opens menu. Fact: yes; gameplay → Pause Menu, Pause Menu → gameplay, Settings → Pause Menu. |
| 4 | Numeric edit commit | Enter/focus loss commits; clamp + snap; Escape cancels | — |
| 5 | Audio label mute | Persist per-channel mute flag; preserve slider value | — |
| 6 | Settings scroll | Mouse wheel + visible draggable scrollbar; Back fixed | — |
| 7 | World grid | Thin subdued isometric grid across playable map; persisted default on | — |
| 8 | Placement assist | Green commits; red snaps to nearest valid footprint within one footprint width | — |
| 9 | Building picking/stats | Rendered sprite ∪ footprint; construction, queue progress/entries, rally, supply grant | — |
| 10 | Command-grid keys | Strict positional QWE / ASD / ZXC; right-click placement cancel | — |
| 11 | Area selection | Bright opaque pure-green 2px border | Also add translucent fill inside selection area; opacity unresolved. |
| 12 | Worker collision exemption | Both units are workers with any `GatherPhase` | Ownership + overlap-exit behavior resolved in Round 2. |

## Round 2 — Navigation and overlap exit

| # | Question | Answer | Precision |
| --- | --- | --- | --- |
| 1 | F10 behavior | Do not add F10; Escape is sufficient | Remove F10 from scope. |
| 2 | Area-selection fill | Pure-green fill at 10% opacity | Keep settled bright opaque pure-green 2px border. |
| 3 | Gather collision ownership | Any ownership; two workers with any `GatherPhase` are exempt | — |
| 4 | Exemption-exit overlap | Push workers apart over bounded ticks; transitional overlaps remain legal | Bound + fallback resolved in Round 3. |

## Round 3 — Separation fallback

| # | Question | Answer | Precision |
| --- | --- | --- | --- |
| 1 | Gradual-separation bound + fallback | Try for 12 ticks; then relocate one worker to nearest legal free center in its connected region | Deterministic rotated priority. |

## Shared understanding

- Goal: polish current RTS menu/settings/HUD/gameplay interactions from `feedback.md`; preserve deterministic, allocation-free frame/sim contracts.
- Settled — affordance: frame discrete interactive controls with idle, hover, pressed, selected, disabled states. Includes modal buttons, commandcard cells, checkbox labels, audio mute labels. Sliders keep slider-specific visuals.
- Settled — menu: add `Close Menu` below `Settings`. Closing clears menu/focus pause, preserves manual Space pause. Escape keeps existing Gameplay ↔ Pause Menu + Settings → Pause Menu behavior. Do not add F10.
- Settled — numeric settings: real draggable sliders update live. Numeric fields accept hand entry. Enter/focus loss commits after clamp + nearest-step snap; Escape cancels edit before menu navigation.
- Settled — audio: MASTER/MUSIC/VOICE/SFX labels toggle persisted mute flags. Slider values remain unchanged while muted; unmute restores exact values. Muted state must be obvious.
- Settled — settings size: wheel scrolling + visible draggable scrollbar. Scroll body clips; Back remains fixed.
- Settled — grid: thin subdued isometric cell grid spans playable map. Persisted toggle defaults on.
- Settled — placement: displayed green candidate always commits. Invalid cursor candidate snaps to nearest valid footprint within one building-footprint width.
- Settled — buildings: click shape = rendered sprite rect ∪ ground footprint. Detail card shows construction state, production queue entries + head progress, rally, supply grant.
- Settled — commandcard: strict positional slot keys `Q/W/E`, `A/S/D`, `Z/X/C`. Right-click remains placement cancel; old dedicated `X` cancel + semantic `A/S/R` production/rally mappings do not survive where they conflict.
- Settled — area selection: bright opaque pure-green 2px border + pure-green 10%-opacity fill.
- Settled — gather collision: any-owner worker pair ignores worker-worker collision only when both workers have `Order::Gather` in any `GatherPhase` (`ToNode`, `Mining`, `Returning`). Static geometry + non-worker/non-gather pair collision remain hard.
- Settled — gather exit: when pair stops qualifying while overlapped, deterministic gradual separation runs up to 12 ticks; transitional overlap stays legal only for that pair. If still merged, rotated priority relocates one worker to nearest legal free center in same connected region. Hard collision resumes after separation.
- Assumptions: existing implemented behavior is extended + integration-verified, not reimplemented. Persisted settings migration preserves schema-1 values while adding grid/mute fields. Grid is render-only; does not alter nav, picking, placement, or state hash. Gradual-separation state is deterministic, hashed if stored, preallocated, zero-allocation per tick.
- Out of scope: F10; HP/armor/combat state; new unit kinds; horde `sim/` collision changes; detailed minimap/fog/terrain systems; performance thresholds; implementation during planning.
