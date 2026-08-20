# T4: Player combat commands

**Plan:** `./artifacts/PLAN_2026_08_17_combat-prototype.md`
**Depends:** T3
**Commit outcome:** Player can order Attack (A + click enemy = target, A + click ground = attack-move), Stop, and right-click an enemy to attack; command card carries the slots positionally; accepted orders emit audio receipts; clicking an enemy shows a read-only card.

## Context (self-contained)

- Goal: Phase 2 Combat Prototype — weapons, damage, turrets, enemy AI. Success = scripted combat run + exit tokens.
- This slice: the player-facing half of combat. Engine combat exists (T3); this wires input → commands → orders → receipts → HUD.
- Out of scope here: turret (T5), HP bars/minimap dots (T6), gate scene/script (T7). Don't touch `sim/`, don't edit tracked scenario/scripts (`assets/scenarios/rts_prototype_v1.ron`, `rts_acceptance_v1.script`, `rts_feedback_polish_v1.script` all stay byte-identical).
- Assumptions in force:
  - Attack accept reuses the `voice_order` receipt family (no new voice bus, no new tracked PCM asset — `src/rts_audio.rs:47 cue_asset_id` pins one asset per `VoiceCue` variant, so the attack family maps onto `VoiceCue::Move`).
  - Command card is positional 3×3, keys `QWE/ASD/ZXC` row-major (`src/rts_input.rs:56-64 KEY_BINDINGS`: `Keycode::A → RtsCommand::ExecuteSlot(3)`, `Keycode::S → ExecuteSlot(4)`; `hud.rs:466 COMMAND_SLOT_KEYS = *b"QWEASDZXC"`).
  - Pointer-owner order fixed (lifecycle, Escape, modal, MENU, minimap, selection icons, command card, HUD background, world); activation = same control down+up (`src/rts_run.rs:412 activation_matches`).
  - Drag box already excludes enemies (`selection.rs:box_select` filters `owner != OWNER_PLAYER`; T2 keeps it). Enemy click-select allowed, read-only, exactly one.
  - **Codebase overrides vs the planning brief (verified by inspection):**
    - There is no `CommandReceipt` type and no `cmd_*` fn anywhere yet; the existing command family is `order_move_group` / `order_gather_group` / `order_build_group` / `issue_context_order_at` on `RtsWorld` (`world.rs:1128/1324/1493/1524`), receipts are `UnitOrderReceipt { id, order: IssuedOrder }` filled into a caller-owned `OrderReceiptBuffer` (`world.rs:63/103`). **This ticket creates `CommandReceipt`** and follows the family's buffer discipline: each `cmd_*` takes `&mut OrderReceiptBuffer` (see Decisions D2).
    - The positional-slot executor is `src/rts_ui.rs:483 execute_slot` → `rts_ui.rs:450 execute_command`, not in `rts_run.rs`.
    - The script vocabulary (`src/rts_script.rs`) already carries everything a fight needs: `key:a`/`key:s` are `ExecuteSlot(3)`/`ExecuteSlot(4)`, plus `lclick`/`rclick`/`move`/`drag`/`key:esc`/`quit`. **No `rts_script.rs` edit is needed** (Decisions D3).
  - T1–T3 have not landed at detailing time; their Outputs are consumed as contracts (quoted under Inputs). Names assumed where a predecessor ticket left them open: `EntityStore::hp(slot) -> u32` accessor (T1 column style matches `amount(slot)`/`progress(slot)`), `max_hp(kind: EntityKind) -> u32` in `entity.rs` (named in T1's Requirements), `weapon(kind: UnitKind) -> Option<Weapon>` in `crates/mmd-engine/src/rts/combat.rs` re-exported as `mmd_engine::rts::weapon` (T3 prefers `rts/combat.rs`). Guard sub-steps 5.8/7.9 below re-verify these after T1–T3 land; adjust paths only, never semantics.

## Decisions (detailer)

- **D1 — card slots: Attack = slot 3 (key `A`), Stop = slot 4 (key `S`).** Not row 1 as the planning sketch guessed. Slots 3/4 are free on every existing card (worker card uses 0/1/2, producer card uses 0+8 — `hud.rs:711 command_slots`), and the untouched positional executor then makes the physical `A` key *be* the attack key and `S` the stop key with zero input-table changes: `key a` → `ExecuteSlot(3)` → `CommandId::Attack`. One binding, one path, live and scripted.
- **D2 — receipt shape.** New in `world.rs`: `CommandReceipt { accepted: usize, rejected: usize, reason: Option<CommandRejectReason> }` + `CommandRejectReason { EmptySelection, NoArmedUnits, NoTarget, Unreachable, NoFormationSpace }`. All three `cmd_*` fns take `&mut OrderReceiptBuffer` (cleared first, one `UnitOrderReceipt` per order written, ascending entity-slot order) — same discipline as `order_build_group`/`issue_context_order_at`. `reason` is set exactly when the whole command was refused (`accepted == 0`); the reject cue keys off `reason.is_some()`.
- **D3 — script tokens: none added.** The exact tokens T7 consumes are named under Outputs. `1:key:a` already parses (`rts_input` test `all_nine_command_keys_map_row_major` proves the mapping).
- **D4 — voice mapping.** `IssuedOrder` gains `Attack | AttackMove | Stop`; `VoiceCue::from_issued` maps all three to `VoiceCue::Move`. No new `VoiceCue` variant → no new tracked WAV, `AudioCounters` (`rts_feedback.rs:271`) counts them as `order_cues` → the exit line's `voice_order`/`voice_reject` move with zero counter-plumbing.
- **D5 — attack-targeting mode = `RtsSession.pending_attack: bool`**, the `pending_rally` pattern (armed by a card command, resolved by the next world left-click, no world state, no hash entry, no cursor visual — rally has none either). Mutual exclusion at arm time: `execute_command` clears `pending_attack` first for every command; `CommandId::Attack` additionally clears `pending_rally` and cancels a pending placement ghost before arming.
- **D6 — A-mode click resolution:** pick an enemy-owned unit → `cmd_attack_target`; any other pick or ground with a valid cell → `cmd_attack_move(cell)`; click off-grid → one `AudioEvent::Reject`. The mode is consumed by the click in every case (one order per A-click, SC1-style).
- **D7 — Escape order in `apply`:** numeric-edit cancel → `pending_attack` cancel → `ui.handle_escape()`. Right-click order: HUD/modal consumed → placement cancel → `pending_attack` cancel → enemy-unit pick with armed selection → `cmd_attack_target` → enemy-only selection → `AudioEvent::Reject` → existing `issue_context_order_at` byte-for-byte.
- **D8 — pick geometry:** `pick_at` drops the `OWNER_PLAYER` filter for **units only** (`selection.rs:367`); buildings keep it (no enemy buildings exist this phase). Nodes already unfiltered.
- **D9 — selection invariant: an enemy id never coexists with anything.** `click_select` already replaces whole. `shift_click_select` gets one rule: if the pick is enemy-owned *or* the current selection holds an enemy, behave as replace instead of toggle.
- **D10 — `cmd_attack_target` internals:** bespoke fn (not `order_group`): validates target (live + `OWNER_ENEMY`, else `NoTarget`), requires ≥1 armed orderable selected (`NoArmedUnits`), plans formation slots **only for the unarmed members** (armed `Order::Attack` carries no goal, so an armed-only selection can never hit `NoFormationSpace`); unarmed shortfall still rejects whole-order (family rule). `cmd_attack_move` reuses `order_group` via a new `GroupTarget::AttackGround(Cell)` — lattice, anchor snap, whole-or-nothing `NoFormationSpace` exactly like `Ground`.
- **D11 — mixed card:** a selection with armed units shows only Attack/Stop (slots 3/4); the worker build buttons keep their existing "no selected non-worker unit" precondition. Worker-only selections stay build-only (no Stop button — the settled "buttons when selection has armed player units" rule; `cmd_stop` itself still idles workers when invoked by any caller).
- **D12 — "any command with enemy selected → reject" scope:** the three `cmd_*` fns reject via `EmptySelection` (an enemy is never `orderable_slot`), and the app right-click path emits one `AudioEvent::Reject` for an enemy-only selection. Positional keys over an empty card stay silent no-ops (existing framing: "Disabled/empty: consumed, no action, no cue", `rts_ui.rs:546`); node/building selections keep their deliberate right-click silence (`world.rs:1546` comment).
- **D13 — icons:** `Prop::IconAttack = 16` (sheet row 4, col 0), `Prop::IconStop = 17` (row 4, col 1) — the props sheet is 4×8 (`assets/sprites/generated/rts/manifest.json`: cols 4, rows 8), rows 4–7 empty today. Placeholder art = flat `draw_hud_icon` squares like every command icon; regenerate with xtask and commit the two changed generated files.

## Requirements

- Engine command surface on `RtsWorld` (`crates/mmd-engine/src/rts/world.rs`):
  - `cmd_attack_target(&mut self, target: EntityId, receipts: &mut OrderReceiptBuffer) -> CommandReceipt` — every selected armed player unit → `Order::Attack { target, field }`; unarmed selected (workers) → `Order::Move` to a formation slot at the target's cell (they walk, never fight). Whole-order rejects: no orderable selection (`EmptySelection`, covers the enemy read-only selection), no armed unit (`NoArmedUnits`), stale/dead/non-enemy target (`NoTarget`), no anchor/field (`Unreachable`), unarmed lattice shortfall (`NoFormationSpace`).
  - `cmd_attack_move(&mut self, cell: Cell, receipts: &mut OrderReceiptBuffer) -> CommandReceipt` — armed → `Order::AttackMove { goal, field }`, unarmed → `Order::Move { goal, field }`, one shared anchor field, distinct lattice slots, insufficient slots rejects the whole order — exactly the `Move` rules via `order_group`.
  - `cmd_stop(&mut self, receipts: &mut OrderReceiptBuffer) -> CommandReceipt` — every selected player unit → `Order::Idle` via `OrderTable::clear` (cancels Move/Gather/Build/Attack/AttackMove; weapon cooldowns are store state and keep ticking; Idle units still auto-acquire per T3).
  - All three: only `OWNER_PLAYER` units act (`orderable_slot`, `world.rs:1285`); receipts ascending by entity slot; accepted → `voice_order` mapping, whole-refusal → `voice_reject`.
- Input layer: `A` (= `ExecuteSlot(3)` → `CommandId::Attack`) arms attack-targeting; next world left-click resolves per D6; right-click or Escape cancels the mode (D7). Right-click on an enemy unit with an armed selection → `cmd_attack_target` directly; right-click ground/node/site semantics unchanged.
- Command card (`hud.rs`): `CommandId::Attack`/`CommandId::Stop` at slots 3/4 when the selection contains armed player units (D1/D11); control-state framing unchanged (`Disabled > Pressed > Hover > Selected > Idle`); positional key executor untouched.
- Enemy pick + card: enemy units pickable (D8); left-click selects exactly that one enemy (D9); card = portrait + 2 text lines: kind label, `HP <cur>/<max>` (via `fmt_ratio`); no command buttons (`command_slots` disqualifies non-player members); any command with it selected rejects (D12). Drag stays player-only.
- Audio: accepted attack/attack-move/stop receipts → `AudioEvent::Voice` batch with `VoiceCue::Move` cues (D4) → `voice_order` counter; whole refusals → `AudioEvent::Reject` → `voice_reject`. Existing tracked scripts never press a key with a soldier selected and contain no enemies (verified line by line), so their pinned counters stand.
- Script tokens: none added (D3).

## Inputs

- **From T3 (contract, verbatim):** `Order::Attack { target: EntityId, field: FieldRef }` tag 4; `Order::AttackMove { goal: FormationGoal, field: FieldRef }` tag 5 (appended in `orders.rs` after `Build` = 3); `weapon(kind: UnitKind) -> Option<Weapon>` (armed = `is_some`; Worker `None`, Soldier `Some`), module `crates/mmd-engine/src/rts/combat.rs`; combat fires only for `Idle`/`Attack`/`AttackMove`; `Attack` chases (field to target), target dead → Idle.
- **From T2 (contract, verbatim):** `OWNER_ENEMY: u8 = 1` in `entity.rs` beside `OWNER_PLAYER = 0`/`OWNER_NEUTRAL = 255`; `UnitKind::Ghoul = 2`; fixture scene `assets/scenarios/fixtures/fixture_rts_combat_v1.ron` (+ `.sha256`) with pre-placed Ghouls.
- **From T1 (contract, verbatim):** `RtsWorld::apply_damage(&mut self, target: EntityId, damage: u32) -> DamageResult`; HP column + `max_hp(kind: EntityKind) -> u32` in `entity.rs`; HP accessor assumed `EntityStore::hp(slot) -> u32` (store accessor style: `amount(slot)`, `progress(slot)`).
- `crates/mmd-engine/src/rts/world.rs` (3678 lines): `IssuedOrder` enum :41; `UnitOrderReceipt` :63; `ContextOrderReason` :75; `OrderReceiptBuffer` :103 (private `push(id, order)` usable within `world.rs`); `GroupTarget` :51; `click_select` :1048; `shift_click_select` :1064; `order_group` :1152 (canonicalize → anchor → acquire once → `FormationScratch::plan` → write orders + receipts ascending); `group_anchor` :1227 (`Ground` → bounds check + `nearest_body_clear_cell`); `eligible_for` :1262; `orderable_slot` :1285 (live + `EntityKind::Unit` + `OWNER_PLAYER`); `issue_context_order_at` :1524 (the `pick_scratch` `std::mem::take` idiom and the "only orderable units count as rejected" comment :1546); `OrderTable::clear(slot)` usage :1466.
- `crates/mmd-engine/src/rts/selection.rs` (456 lines): `pick_at` :318 doc + body; the unit owner filter is line 367 `let mine = world.entities().owner(slot) == OWNER_PLAYER;` with the unit arm `(mine && unit_pick_contains(...))`; two-tier ranking (exact pickshapes beat building sprite quads — post-af16e7c); `box_select` :~430 player filter stays.
- `crates/mmd-engine/src/rts/hud.rs` (1933 lines): `kind_label` :652; `CommandId` :666; `CommandSlot`/`EMPTY_SLOT` :677; `command_icon` :688; `command_slots` :711 (worker card → slots 0/1/2; single finished HQ/Barracks → 0 + 8; anything else disabled; **no owner check today**); `portrait_source` :906; `push_detail_text` :943 (line 1 = `kind_label`, then per-kind match); `fmt_ratio` :~638; `COMMAND_SLOT_KEYS` :466; imports at :14-20.
- `crates/mmd-engine/src/rts/pack.rs`: `Prop` :57 (`IconSetRally = 15` is last), `prop_uv` :77 (`row = i/4, col = i%4`).
- `crates/mmd-engine/src/rts/mod.rs`: `pub use` lists — `pick_at`, `Pick`, `OrderReceiptBuffer`, `IssuedOrder`, `ContextOrderResult` already exported; add new symbols here.
- `crates/mmd-engine/src/rts/formation.rs`: `FormationError { NoUnits, NoTarget, Unreachable, NoFormationSpace }` :90; `FormationScratch::begin/push_unit/plan/unit(i)/slot(i)` (plan writes slots in push order, ascending).
- `src/rts_input.rs` (256 lines): `KEY_BINDINGS` :53-64 — **no edit needed**; banner :131 already says "QWE/ASD/ZXC card".
- `src/rts_run.rs` (2885 lines): `RtsSession` :131 (`receipts: OrderReceiptBuffer` :169, `pending_rally` :172, ctor :206 with inits :224-225); `activate_world_left` :433 (rally → placement → `click_select` chain); `apply` :588 (`Escape` arm :591, `ExecuteSlot` :~600, `RightClick` arm :685: owner-consumed → placement-cancel → `issue_context_order_at` + `order_cues`/`reject_cue`); imports :76-109 (`mmd_engine::rts::{...}` block :83, `crate::rts_feedback::{...}` :95); test mod :2023 (`route`/`tracked_world`/`select_a_unit` :2126-2168, `RtsSession::for_test()` :307 returns `(session, FakeSinkHandle)`; `session.audio_counters.order_cues/reject` assertions pattern :2225).
- `src/rts_ui.rs` (2944 lines): `execute_command` :450 (`CommandId::SetRally` arm :471 — the "arm a pending action" precedent); `execute_slot` :483; `handle_hud_click` :497 (`HudHit::CommandSlot` → `execute_slot` + `UiCue::CommandGrid` :543).
- `src/rts_feedback.rs` (993 lines): `VoiceCue` :57 + `from_issued` :66-71; `AudioEvent` :166 (`Voice(VoiceBatch)`, `Reject`); `AudioCounters::record` :262 (`VoiceCue::Move | Gather | Build → order_cues` :271); `snapshot_selected_units` :459 (player-unit filter — enemy selection never voices Select); `order_cues` :492; `reject_cue` :507; engine imports :33.
- `src/rts_script.rs` (461 lines): grammar table :11-19 — read-only this ticket.
- `mmd_engine::testkit::RtsHarness` (`crates/mmd-engine/src/testkit/rts.rs`): `RtsHarness::path(p).build()`, `::spec(spec)`, `world()/world_mut()/step_exact(n)/state_hash()`; test idiom from `crates/mmd-engine/tests/rts_hud.rs:585-598`: `h.world_mut().entities_mut().spawn(EntityKind::Building(BuildingKind::Barracks), OWNER_PLAYER, [200.0, 210.0])` + `h.world_mut().selection_mut().insert(id)` — same works for `EntityKind::Unit(UnitKind::Soldier)` and `(UnitKind::Ghoul, OWNER_ENEMY)` at body-clear cells.
- `xtask/src/placeholder_art.rs`: icon colors :64-71; `draw_props_cell` :248 (command-icon arms :312-317, `draw_hud_icon` = inset flat square); regen CLI `cargo run -p xtask -- atlases`, verify `-- atlases --check` (`xtask/src/atlases.rs:449 run_atlases`). GPU goldens pin `assets/sprites/generated/manifest.json` (zombie atlases), **not** `assets/sprites/generated/rts/manifest.json` (`render/golden.rs:545`) — regenerating the rts props sheet cannot break goldens.
- Merge gate (docs/05-testing.md `## Required merge gate`): fmt, `cargo test --workspace --locked`, clippy `-D warnings`, `nix flake check`, xtask `bootstrap/shaders/atlases/audio --check`, three `run` smokes, `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`.

## TDD

1. **Red** — failing tests below.
2. **Green** — min code.
3. **Refactor** — keep green.

## Test plan

Engine — new file `crates/mmd-engine/tests/rts_combat_commands.rs` (module doc: "T4: the player command surface over T3's combat"). Setup helper: `RtsHarness::path("assets/scenarios/fixtures/fixture_rts_combat_v1.ron")` (path relative to `CARGO_MANIFEST_DIR` of `mmd-engine` → use `Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/scenarios/fixtures/fixture_rts_combat_v1.ron")` — copy the exact join the existing `testkit::rts_scene_path` uses; inspect after T2 lands) or `RtsHarness::scene()` + spawned units where no enemy is needed; spawn soldiers/ghouls via `entities_mut().spawn(...)` at body-clear cells, select via `selection_mut().insert(id)`, one `OrderReceiptBuffer::new()` per test.

| Test (exact fn name) | Setup | Exact assertions |
| ---- | ----- | ------ |
| `attack_target_orders_armed_selection` | 2 spawned Soldiers selected, 1 Ghoul; `cmd_attack_target(ghoul, &mut r)` | receipt `{accepted: 2, rejected: 0, reason: None}`; `world.order_of(s).unwrap().tag() == 4` for both; `r.as_slice()` = 2 receipts, `order == IssuedOrder::Attack`, ascending ids |
| `attack_target_walks_then_kills` | Soldier selected far from Ghoul (out of Soldier range), `cmd_attack_target`, `step_exact` until Ghoul dead (bound the loop, e.g. 3600 ticks) | Ghoul id resolves `None`; soldier `order_of(...) == Some(Order::Idle)`; `world.kills() == 1` |
| `attack_move_formation_semantics` | 4 Soldiers selected, `cmd_attack_move(cell, &mut r)` | `accepted == 4`; 4 distinct `FormationGoal::slot` cells; all orders tag 5; receipts `IssuedOrder::AttackMove` |
| `workers_in_selection_move_dont_fight` | 1 Worker + 1 Soldier selected, `cmd_attack_move(cell)` | worker order tag 1 (`Move`), receipt `IssuedOrder::Move`; soldier tag 5, receipt `IssuedOrder::AttackMove`; `accepted == 2` |
| `attack_target_walks_workers_instead` | 1 Worker + 1 Soldier selected, `cmd_attack_target(ghoul)` | worker order tag 1 with goal anchored at the Ghoul's cell; soldier tag 4; `accepted == 2` |
| `attack_target_rejects_unarmed_dead_and_missing` | (a) workers-only selection + live Ghoul; (b) soldiers + despawned Ghoul id; (c) soldiers + `cmd_attack_target(soldier_id)` (player-owned target) | (a) `reason == Some(NoArmedUnits)`; (b)+(c) `reason == Some(NoTarget)`; all: `accepted == 0`, no order changed (`order_of` unchanged), `r.as_slice().is_empty()` |
| `stop_idles_and_cancels` | gathering Worker (via `order_gather`) + attacking Soldier (via `cmd_attack_target`) both selected; `cmd_stop(&mut r)` | both `order_of == Some(Order::Idle)`; receipt `{accepted: 2, rejected: 0, reason: None}`; receipts `IssuedOrder::Stop` |
| `enemy_selection_rejects_commands` | one Ghoul selected via `select_only`; call each of the three cmds | every receipt `{accepted: 0, reason: Some(EmptySelection)}`; Ghoul order untouched |
| `command_receipts_do_not_grow_the_buffer` | reuse one buffer across the three cmds with a full selection | `r.capacity()` constant (matches `OrderReceiptBuffer` reserve discipline) |

Engine — `crates/mmd-engine/tests/rts_selection.rs` (append):

| Test | Setup | Assertions |
| ---- | ----- | ------ |
| `enemy_unit_is_pickable_and_click_selects_exactly_one` | spawn Ghoul, project its ground point via `sprite_screen_rect` centre, `click_select` | `Pick::Unit(ghoul)`; `selection().ids() == [ghoul]`; repeat with a player selection active first — selection replaced |
| `shift_click_never_mixes_enemy_and_player` | player worker selected, shift-click Ghoul; then Ghoul selected, shift-click worker | after each: `selection().len() == 1`, sole id owner matches the last click |
| `drag_box_still_excludes_enemies` | drag over worker + Ghoul | selection = worker only |

Engine — `crates/mmd-engine/tests/rts_hud.rs` (append to the `command_slots` section):

| Test | Setup | Assertions |
| ---- | ----- | ------ |
| `card_shows_attack_stop_for_armed` | (a) spawned Soldier selected; (b) Soldier+Worker; (c) workers only | (a)+(b): `slots[3].command == Some(CommandId::Attack)`, `slots[4].command == Some(CommandId::Stop)`, both enabled, every other slot `None`; (c): slots 0/1/2 build commands, 3/4 `None` (existing `worker_card_uses_stable_three_build_slots` must stay green) |
| `enemy_selection_shows_no_commands` | Ghoul selected | all 9 `command.is_none()` |
| `enemy_card_two_lines_kind_and_hp` | Ghoul selected, `pack_hud` into an `RtsFrame`, scan the font group with the existing glyph-scan helper in this file | text contains `GHOUL` and `HP 30/30` (T2 stats: max 30); after `apply_damage(ghoul, 6)` → `HP 25/30` (armor 0, 5+floor→ per T1 `max(1, 6-0)=6` → `HP 24/30`; compute from T1 rules at impl time and assert the exact string) |

App — `src/rts_run.rs` `mod tests` (use `RtsSession::for_test()`, `apply`, a fixture-loaded world `RtsWorld::load("assets/scenarios/fixtures/fixture_rts_combat_v1.ron")` + `entities_mut()` spawns; follow `select_a_unit` :2153 pattern):

| Test | Drive | Assertions |
| ---- | ----- | ------ |
| `attack_key_arms_and_ground_click_attack_moves` | select spawned Soldier; `apply(.., RtsCommand::ExecuteSlot(3))`; `apply(.., RtsCommand::LeftClick(world_point))` | after key: `session.pending_attack`; after click: soldier order tag 5, `session.audio_counters.order_cues == 1`, `reject == 0`, `pending_attack == false` |
| `attack_click_on_enemy_targets_it` | armed selection; `ExecuteSlot(3)`; `LeftClick` on Ghoul's sprite point | soldier order tag 4 (`Order::Attack`), `order_cues == 1` |
| `right_click_enemy_attacks_directly` | armed selection, no mode; `RightClick` on Ghoul point | order tag 4; `order_cues == 1`; no mode armed |
| `escape_and_right_click_cancel_attack_mode` | arm mode; (a) `Escape`; (b) re-arm, `RightClick` on ground | mode cleared both times; no order issued (`order_cues == 0`); (a): `session.ui` page still `Gameplay` (Escape consumed by the cancel, menu did not open) |
| `stop_key_emits_voice_order` | gathering worker + soldier selected (armed present → card has Stop); `ExecuteSlot(4)` | both Idle; `order_cues == 1` batch counted (2 cues), `reject == 0` |
| `enemy_selected_right_click_rejects` | Ghoul selected via click; `RightClick` on ground | `session.audio_counters.reject == 1`; no order; Ghoul still selected |
| `attack_reject_beeps` | workers-only selected cannot arm (slot 3 empty → `ExecuteSlot(3)` no-op, assert `pending_attack == false`); then armed selection + `cmd` path with dead target via `RightClick` on a ghoul killed first | `reject` counter increments exactly once for the dead-target click |

App — `src/rts_feedback.rs` `mod tests` (append): `attack_family_receipts_voice_as_move_orders` — `order_cues(&[receipt(1, IssuedOrder::Attack), receipt(2, IssuedOrder::AttackMove), receipt(3, IssuedOrder::Stop)])` → batch of 3, every cue `VoiceCue::Move`; `AudioCounters::record` on that batch → `order_cues == 3`. `command_reject_cue_fires_on_reason_only` — `reason: None` → `None`; `reason: Some(EmptySelection)` → `Some(AudioEvent::Reject)`.

## Impl steps

- [ ] 1. **Engine receipt types + attack-move plumbing (`crates/mmd-engine/src/rts/world.rs`)**
  - [ ] 1.1 After the `ContextOrderResult` struct (:96), insert the two new pub types exactly:
    ```rust
    /// Why a `cmd_*` player command was refused whole. Every variant leaves
    /// the world untouched.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum CommandRejectReason {
        /// The selection holds no live player-owned unit.
        EmptySelection,
        /// Attack-target needs at least one armed unit in the selection.
        NoArmedUnits,
        /// The attack target is stale, dead, or not enemy-owned.
        NoTarget,
        /// No navigation field could be built to the command's anchor cell.
        Unreachable,
        /// Fewer legal formation slots than the walking members need —
        /// whole-order refusal, the same rule Move has always had.
        NoFormationSpace,
    }

    /// The outcome of one [`RtsWorld::cmd_attack_target`],
    /// [`RtsWorld::cmd_attack_move`] or [`RtsWorld::cmd_stop`].
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct CommandReceipt {
        /// Orders actually written — one receipt each in the caller's buffer,
        /// ascending by entity slot.
        pub accepted: usize,
        /// Selected orderable units this command left unordered.
        pub rejected: usize,
        /// Set exactly when the whole command was refused (`accepted == 0`).
        pub reason: Option<CommandRejectReason>,
    }
    ```
  - [ ] 1.2 `IssuedOrder` (:41): append `Attack,`, `AttackMove,`, `Stop,` after `Build`.
  - [ ] 1.3 `GroupTarget` (:51): append variant `/// Attack-move: armed members fight on the way, unarmed members just walk.` `AttackGround(Cell),`.
  - [ ] 1.4 `group_anchor` (:1227): change the `GroupTarget::Ground(cell)` arm's pattern to `GroupTarget::Ground(cell) | GroupTarget::AttackGround(cell)` (body unchanged).
  - [ ] 1.5 `eligible_for` (:1262): change `GroupTarget::Ground(_) => true,` to `GroupTarget::Ground(_) | GroupTarget::AttackGround(_) => true,`.
  - [ ] 1.6 `order_group` write-match (:1200, the `let (order, issued) = match target {` block): add an arm before `GroupTarget::Site`:
    ```rust
    GroupTarget::AttackGround(_) => {
        let armed = matches!(
            self.entities.kind(slot),
            EntityKind::Unit(k) if weapon(k).is_some()
        );
        if armed {
            (Order::AttackMove { goal, field }, IssuedOrder::AttackMove)
        } else {
            (Order::Move { goal, field }, IssuedOrder::Move)
        }
    }
    ```
    and add `use super::combat::weapon;` to the `world.rs` import block (adjust the module path only if T3 landed `weapon` elsewhere — grep `pub fn weapon` first).
  - [ ] 1.7 New pub fn after `order_move_group` (:1135):
    ```rust
    /// Player command: attack-move the current selection to `cell`.
    ///
    /// Armed members take [`Order::AttackMove`], unarmed ones a plain
    /// [`Order::Move`]; lattice, shared anchor field and the whole-or-nothing
    /// `NoFormationSpace` rule are exactly Move's.
    pub fn cmd_attack_move(
        &mut self,
        cell: Cell,
        receipts: &mut OrderReceiptBuffer,
    ) -> CommandReceipt {
        let mut scratch = std::mem::take(&mut self.pick_scratch);
        scratch.clear();
        scratch.extend_from_slice(self.selection.ids());
        let selected = scratch
            .iter()
            .filter(|&&id| self.orderable_slot(id).is_some())
            .count();
        let outcome = self.order_group(&scratch, GroupTarget::AttackGround(cell), Some(receipts));
        self.pick_scratch = scratch;
        match outcome {
            Ok(n) => CommandReceipt {
                accepted: n,
                rejected: selected - n,
                reason: None,
            },
            Err(e) => CommandReceipt {
                accepted: 0,
                rejected: selected,
                reason: Some(match e {
                    FormationError::NoUnits | FormationError::NoTarget => {
                        CommandRejectReason::EmptySelection
                    }
                    FormationError::Unreachable => CommandRejectReason::Unreachable,
                    FormationError::NoFormationSpace => CommandRejectReason::NoFormationSpace,
                }),
            },
        }
    }
    ```
- [ ] 2. **Engine `cmd_attack_target` (`world.rs`, directly after 1.7's fn)**
  - [ ] 2.1 Add the bespoke fn (uses `OWNER_ENEMY` — extend the `super::entity::{...}` import with `OWNER_ENEMY`):
    ```rust
    /// Player command: every selected armed player unit chases and attacks
    /// `target`; unarmed selected units walk to a formation slot at the
    /// target's cell instead — they never fight. Whole or nothing, like the
    /// rest of the command family.
    pub fn cmd_attack_target(
        &mut self,
        target: EntityId,
        receipts: &mut OrderReceiptBuffer,
    ) -> CommandReceipt {
        receipts.clear();
        let mut scratch = std::mem::take(&mut self.pick_scratch);
        scratch.clear();
        scratch.extend_from_slice(self.selection.ids());
        let selected = scratch
            .iter()
            .filter(|&&id| self.orderable_slot(id).is_some())
            .count();
        let reject = |reason| CommandReceipt {
            accepted: 0,
            rejected: selected,
            reason: Some(reason),
        };

        if selected == 0 {
            self.pick_scratch = scratch;
            return reject(CommandRejectReason::EmptySelection);
        }
        let target_ok = self
            .entities
            .slot(target)
            .is_some_and(|s| self.entities.owner(s) == OWNER_ENEMY);
        if !target_ok {
            self.pick_scratch = scratch;
            return reject(CommandRejectReason::NoTarget);
        }

        // Partition ascending by slot: armed fight, unarmed only walk — only
        // the walkers need lattice slots, so an armed-only selection can
        // never be refused for formation space.
        let mut armed_count = 0usize;
        self.formation.begin();
        for slot in 0..self.entities.slot_count() {
            let Some(id) = self.entities.id_at(slot) else {
                continue;
            };
            if !scratch.contains(&id) || self.orderable_slot(id).is_none() {
                continue;
            }
            if matches!(self.entities.kind(slot), EntityKind::Unit(k) if weapon(k).is_some()) {
                armed_count += 1;
            } else {
                self.formation.push_unit(id);
            }
        }
        if armed_count == 0 {
            self.pick_scratch = scratch;
            return reject(CommandRejectReason::NoArmedUnits);
        }

        let target_slot = self.entities.slot(target).expect("checked live above");
        let pos = self.entities.position(target_slot);
        let target_cell = Cell {
            x: pos[0] as u32,
            y: pos[1] as u32,
        };
        let Some(anchor) = nearest_body_clear_cell(&self.static_nav, target_cell) else {
            self.pick_scratch = scratch;
            return reject(CommandRejectReason::Unreachable);
        };
        let Ok(field) = self.nav.acquire(anchor) else {
            self.pick_scratch = scratch;
            return reject(CommandRejectReason::Unreachable);
        };
        if self.formation.len() > 0
            && let Err(e) = self.formation.plan(
                &self.static_nav,
                &self.entities,
                &self.nav,
                field.slot,
                anchor,
            )
        {
            self.pick_scratch = scratch;
            return reject(match e {
                FormationError::NoFormationSpace => CommandRejectReason::NoFormationSpace,
                _ => CommandRejectReason::Unreachable,
            });
        }

        let mut accepted = 0usize;
        let mut walker = 0usize;
        for slot in 0..self.entities.slot_count() {
            let Some(id) = self.entities.id_at(slot) else {
                continue;
            };
            if !scratch.contains(&id) || self.orderable_slot(id).is_none() {
                continue;
            }
            let armed =
                matches!(self.entities.kind(slot), EntityKind::Unit(k) if weapon(k).is_some());
            let (order, issued) = if armed {
                (Order::Attack { target, field }, IssuedOrder::Attack)
            } else {
                debug_assert_eq!(self.formation.unit(walker), id, "plan order is push order");
                let goal = FormationGoal {
                    anchor,
                    slot: self.formation.slot(walker),
                };
                walker += 1;
                (Order::Move { goal, field }, IssuedOrder::Move)
            };
            self.orders.set(slot, order);
            receipts.push(id, issued);
            accepted += 1;
        }
        self.pick_scratch = scratch;
        CommandReceipt {
            accepted,
            rejected: selected - accepted,
            reason: None,
        }
    }
    ```
    (`nearest_body_clear_cell` is already imported from `super::formation` at the top of `world.rs`.)
- [ ] 3. **Engine `cmd_stop` (`world.rs`, after 2.1)**
  - [ ] 3.1 Add:
    ```rust
    /// Player command: every selected player unit stops — [`Order::Idle`],
    /// cancelling gather, build, move and both attack orders in place.
    /// Weapon cooldowns are store state and keep ticking; an Idle armed unit
    /// still auto-acquires (T3's firing rules).
    pub fn cmd_stop(&mut self, receipts: &mut OrderReceiptBuffer) -> CommandReceipt {
        receipts.clear();
        let mut scratch = std::mem::take(&mut self.pick_scratch);
        scratch.clear();
        scratch.extend_from_slice(self.selection.ids());
        let mut accepted = 0usize;
        for slot in 0..self.entities.slot_count() {
            let Some(id) = self.entities.id_at(slot) else {
                continue;
            };
            if !scratch.contains(&id) || self.orderable_slot(id).is_none() {
                continue;
            }
            self.orders.clear(slot);
            receipts.push(id, IssuedOrder::Stop);
            accepted += 1;
        }
        self.pick_scratch = scratch;
        if accepted == 0 {
            return CommandReceipt {
                accepted: 0,
                rejected: 0,
                reason: Some(CommandRejectReason::EmptySelection),
            };
        }
        CommandReceipt {
            accepted,
            rejected: 0,
            reason: None,
        }
    }
    ```
  - [ ] 3.2 `crates/mmd-engine/src/rts/mod.rs`, the `pub use world::{...}` list: add `CommandReceipt, CommandRejectReason` (keep alphabetical order in the list). Verify `OWNER_ENEMY` is in the `pub use entity::{...}` list (T2 should have added it; if missing, add it here).
- [ ] 4. **Engine enemy pick + selection invariant**
  - [ ] 4.1 `selection.rs:367` — in `pick_at`, change the unit arm from
    `EntityKind::Unit(kind) => (mine && unit_pick_contains(view, ground, kind.body_radius_cells(), screen)).then_some(PickTier::Exact),`
    to
    `EntityKind::Unit(kind) => unit_pick_contains(view, ground, kind.body_radius_cells(), screen).then_some(PickTier::Exact),`
    and update `pick_at`'s doc list: `**Unit** (any owner — an enemy is clickable for the read-only card and the attack command)`. Keep `mine` for the building arms (`let mine = ...` stays; clippy will not flag it since buildings still read it).
  - [ ] 4.2 `world.rs:1064 shift_click_select` — replace the `Pick::Unit(id) | Pick::Building(id) | Pick::Node(id)` arm body with:
    ```rust
    Pick::Unit(id) | Pick::Building(id) | Pick::Node(id) => {
        let enemy = |eid: EntityId| {
            self.entities
                .slot(eid)
                .is_some_and(|s| self.entities.owner(s) == OWNER_ENEMY)
        };
        // An enemy selection is read-only and always exactly one: additive
        // refinement never mixes owners, in either direction.
        if enemy(id) || self.selection.ids().iter().any(|&sel| enemy(sel)) {
            self.selection.clear();
            self.selection.insert(id);
        } else {
            self.selection.toggle(id);
        }
    }
    ```
    (`click_select` :1048 needs no change — replace-with-pick already yields exactly one.)
- [ ] 5. **Engine HUD: card slots, icons, enemy card (`hud.rs`, `pack.rs`, xtask)**
  - [ ] 5.1 `pack.rs:57 Prop`: append `IconAttack = 16,` and `IconStop = 17,` after `IconSetRally = 15`. (`prop_uv` :77 already handles rows beyond 3 — sheet is 4×8.)
  - [ ] 5.2 `hud.rs:666 CommandId`: append `Attack,` and `Stop,` after `SetRally`.
  - [ ] 5.3 `hud.rs:688 command_icon`: add arms `CommandId::Attack => Prop::IconAttack,` and `CommandId::Stop => Prop::IconStop,`.
  - [ ] 5.4 `hud.rs:711 command_slots` — rewrite the tally loop and branch chain (keep the fn signature and the `out` array):
    ```rust
    let mut worker_count = 0u32;
    let mut armed_count = 0u32;
    let mut building_count = 0u32;
    let mut disqualified = false;
    let mut finished_building: Option<BuildingKind> = None;

    for &id in world.selection().ids() {
        let Some(slot) = store.slot(id) else {
            continue; // stale — not counted either way
        };
        if store.owner(slot) != OWNER_PLAYER {
            // The read-only enemy card offers no command at all.
            disqualified = true;
            continue;
        }
        match store.kind(slot) {
            EntityKind::Unit(UnitKind::Worker) => worker_count += 1,
            EntityKind::Unit(kind) if weapon(kind).is_some() => armed_count += 1,
            EntityKind::Unit(_) => disqualified = true,
            EntityKind::Building(kind) => {
                building_count += 1;
                if store.progress_target(slot) == 0 {
                    finished_building = Some(kind);
                } else {
                    disqualified = true;
                }
            }
            _ => disqualified = true,
        }
    }

    let mut out = [EMPTY_SLOT; 9];

    if armed_count > 0 && building_count == 0 && !disqualified {
        // Slot 3 = key A, slot 4 = key S (row-major QWE/ASD/ZXC): the
        // positional executor is what makes A the attack key.
        out[3] = CommandSlot {
            command: Some(CommandId::Attack),
            enabled: true,
        };
        out[4] = CommandSlot {
            command: Some(CommandId::Stop),
            enabled: true,
        };
    } else if worker_count > 0 && building_count == 0 && !disqualified {
        /* existing build block for slots 0/1/2, unchanged */
    } else if worker_count == 0 && armed_count == 0 && building_count == 1 && !disqualified {
        /* existing produce/rally block, unchanged */
    }

    out
    ```
    Update the fn's doc list with the new armed-card case. Extend hud.rs imports: `use super::combat::weapon;` and add `OWNER_PLAYER` (and `OWNER_ENEMY` for 5.5) to the `super::entity::{...}` import at :16.
  - [ ] 5.5 `hud.rs:943 push_detail_text` — insert the enemy branch between the kind-label line (`y += PANEL_LINE_PX;`) and `match kind {`:
    ```rust
    // Read-only enemy card: exactly two lines — the kind and HP cur/max.
    if store.owner(slot) == OWNER_ENEMY {
        let mut cx = x;
        cx += push_text(font, "HP ", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
        let s = fmt_ratio(&mut buf, store.hp(slot), max_hp(kind));
        push_text(font, s, [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
        return;
    }
    ```
    (`store.hp(slot)`/`max_hp(kind)` are T1's names — confirm the exact accessor T1 landed with `grep -n "fn hp\|fn max_hp" crates/mmd-engine/src/rts/entity.rs` and adjust the two call sites only. Add `max_hp` to the hud.rs entity import.)
  - [ ] 5.6 `xtask/src/placeholder_art.rs:71` — after `ICON_SET_RALLY_COLOR` add:
    ```rust
    const ICON_ATTACK_COLOR: [u8; 4] = [220, 60, 40, 255];
    const ICON_STOP_COLOR: [u8; 4] = [200, 200, 210, 255];
    ```
    and in `draw_props_cell` (:317, after the `(3, 3)` arm) add:
    ```rust
    (4, 0) => draw_hud_icon(tile, ICON_ATTACK_COLOR),
    (4, 1) => draw_hud_icon(tile, ICON_STOP_COLOR),
    ```
  - [ ] 5.7 Regenerate + verify the tracked sheets:
    ```sh
    cargo run -p xtask -- atlases
    git status --porcelain assets/sprites/   # expect exactly: rts/props.png, rts/manifest.json
    cargo run -p xtask -- atlases --check
    ```
    Commit both changed files with the code. (GPU goldens pin `assets/sprites/generated/manifest.json` — the zombie family — not the rts one; `render/golden.rs:545`.)
  - [ ] 5.8 Guards after T1–T3 land (compile-forced, verify only): `hud.rs kind_label` has a `UnitKind::Ghoul` arm (if T2 left it out, add `EntityKind::Unit(UnitKind::Ghoul) => "GHOUL",`); `portrait_source` :906 has a Ghoul arm cropping whatever sheet T2/T3 draw Ghouls from; the `UnitKind::Worker/Soldier` queue-label match at `hud.rs:1027` still compiles (producers never queue Ghouls — arm can be `"G"` or unreachable per T2's choice). Run `cargo check -p mmd-engine` and fix only what the compiler names.
- [ ] 6. **App feedback mapping (`src/rts_feedback.rs`)**
  - [ ] 6.1 `from_issued` (:66): add arm
    ```rust
    // The attack family voices as Move: no new tracked voice asset, and the
    // exit line's voice_order counter moves with it.
    IssuedOrder::Attack | IssuedOrder::AttackMove | IssuedOrder::Stop => Self::Move,
    ```
  - [ ] 6.2 After `reject_cue` (:509) add:
    ```rust
    /// The single reject cue for one `cmd_*` command, when it was refused
    /// whole. Per action, never per unit — same rule as [`reject_cue`].
    pub fn command_reject_cue(receipt: &CommandReceipt) -> Option<AudioEvent> {
        receipt.reason.is_some().then_some(AudioEvent::Reject)
    }
    ```
    and add `CommandReceipt` to the `mmd_engine::rts::{...}` import at :33.
- [ ] 7. **App input: session mode, executor, routing (`src/rts_run.rs`, `src/rts_ui.rs`)**
  - [ ] 7.1 `rts_run.rs` `RtsSession`: after `pending_rally` (:172) add field
    ```rust
    /// Armed by `CommandId::Attack`: the *next* world left-click resolves as
    /// an attack (enemy under cursor) or an attack-move (ground). Right-click
    /// or Escape disarms it. The `pending_rally` pattern: session state, not
    /// world state, never hashed.
    pub(crate) pending_attack: bool,
    ```
    and `pending_attack: false,` in `with_sink` (:225, beside `pending_rally: None`).
  - [ ] 7.2 `rts_run.rs` imports: extend the `mmd_engine::rts::{...}` block (:83) with `CommandReceipt, CommandRejectReason, OWNER_ENEMY, Pick, pick_at, weapon` and the `crate::rts_feedback::{...}` block (:95) with `command_reject_cue`.
  - [ ] 7.3 `rts_run.rs`, below `find_builder` (:348), add the three helpers:
    ```rust
    /// Whether the selection holds at least one live player unit with a weapon.
    fn selection_has_armed_player_unit(world: &RtsWorld) -> bool {
        let store = world.entities();
        world.selection().ids().iter().any(|&id| {
            store.slot(id).is_some_and(|slot| {
                store.owner(slot) == OWNER_PLAYER
                    && matches!(store.kind(slot), EntityKind::Unit(k) if weapon(k).is_some())
            })
        })
    }

    /// Whether the selection is the read-only enemy card: non-empty and
    /// every member enemy-owned.
    fn selection_is_enemy_only(world: &RtsWorld) -> bool {
        let store = world.entities();
        let ids = world.selection().ids();
        !ids.is_empty()
            && ids
                .iter()
                .all(|&id| store.slot(id).is_some_and(|s| store.owner(s) == OWNER_ENEMY))
    }

    /// Voice one command receipt: the accepted receipts as one order batch,
    /// a whole-command refusal as one Reject.
    pub(crate) fn emit_command_feedback(session: &mut RtsSession, receipt: CommandReceipt) {
        if let Some(batch) = order_cues(session.receipts.as_slice()) {
            session.emit_audio(AudioEvent::Voice(batch));
        }
        if let Some(event) = command_reject_cue(&receipt) {
            session.emit_audio(event);
        }
    }

    /// Run `cmd_stop` for the current selection and voice its receipts —
    /// shared by the card button and the positional key.
    pub(crate) fn execute_stop(world: &mut RtsWorld, session: &mut RtsSession) {
        let receipt = world.cmd_stop(&mut session.receipts);
        emit_command_feedback(session, receipt);
    }

    /// Resolve the armed attack-targeting click: an enemy unit under the
    /// cursor is attacked directly; anything else attack-moves to the
    /// clicked cell; off the grid is a refusal. The mode is consumed either
    /// way.
    fn resolve_attack_click(world: &mut RtsWorld, session: &mut RtsSession, p: [f32; 2]) {
        let view = world.iso_view();
        let enemy_unit = match pick_at(world, &view, p) {
            Pick::Unit(id)
                if world
                    .entities()
                    .slot(id)
                    .is_some_and(|s| world.entities().owner(s) == OWNER_ENEMY) =>
            {
                Some(id)
            }
            _ => None,
        };
        let receipt = if let Some(id) = enemy_unit {
            world.cmd_attack_target(id, &mut session.receipts)
        } else {
            let width = world.scenario().width();
            let height = world.scenario().height();
            match view.cell_at(p[0], p[1], width, height) {
                Some(cell) => world.cmd_attack_move(cell, &mut session.receipts),
                None => {
                    session.receipts.clear();
                    CommandReceipt {
                        accepted: 0,
                        rejected: 0,
                        reason: Some(CommandRejectReason::Unreachable),
                    }
                }
            }
        };
        emit_command_feedback(session, receipt);
    }
    ```
  - [ ] 7.4 `activate_world_left` (:433): make the armed mode the first branch:
    ```rust
    fn activate_world_left(world: &mut RtsWorld, session: &mut RtsSession, p: [f32; 2]) {
        if session.pending_attack {
            session.pending_attack = false;
            resolve_attack_click(world, session, p);
        } else if let Some(building) = session.pending_rally.take() {
    ```
    (rest of the chain unchanged).
  - [ ] 7.5 `apply`'s `RtsCommand::Escape` arm (:591): insert the mode cancel between the numeric-edit branch and `session.ui.handle_escape()`:
    ```rust
    } else if session.pending_attack {
        // Escape disarms attack-targeting; the menu does not open on the
        // same press.
        session.pending_attack = false;
    } else {
    ```
  - [ ] 7.6 `apply`'s `RtsCommand::RightClick(p)` arm (:685): extend the branch chain — after the placement-cancel branch and before the existing context-order `else`, insert:
    ```rust
    } else if session.pending_attack {
        // Right click disarms attack-targeting instead of ordering — the
        // same shape as the placement ghost.
        session.pending_attack = false;
    } else {
        let view = world.iso_view();
        let enemy_unit = match pick_at(world, &view, p) {
            Pick::Unit(id)
                if world
                    .entities()
                    .slot(id)
                    .is_some_and(|s| world.entities().owner(s) == OWNER_ENEMY) =>
            {
                Some(id)
            }
            _ => None,
        };
        if let Some(id) = enemy_unit
            && selection_has_armed_player_unit(world)
        {
            // Direct attack: right click on an enemy bypasses the A-mode.
            let receipt = world.cmd_attack_target(id, &mut session.receipts);
            emit_command_feedback(session, receipt);
        } else if selection_is_enemy_only(world) {
            // The read-only enemy selection: every command is a refusal.
            session.emit_audio(AudioEvent::Reject);
        } else {
            /* existing block, byte for byte: let result = world
               .issue_context_order_at(&view, p, &mut session.receipts);
               order_cues + reject_cue emission */
        }
    }
    ```
    (the existing `let view = world.iso_view();` inside the old `else` moves up as shown; `issue_context_order_at` reuses it).
  - [ ] 7.7 `rts_ui.rs:450 execute_command`: first statement of the fn body: `// Any card command supersedes a pending attack-targeting mode.` `session.pending_attack = false;` — then add two arms after `CommandId::SetRally`:
    ```rust
    CommandId::Attack => {
        // Arms the targeting mode rather than acting immediately: the next
        // world left-click resolves it (enemy = target, ground = attack-move).
        world.cancel_placement();
        session.pending_rally = None;
        session.pending_attack = true;
    }
    CommandId::Stop => {
        crate::rts_run::execute_stop(world, session);
    }
    ```
  - [ ] 7.8 `src/main.rs`/module tree: no change (`rts_run` is already a crate module; `execute_stop`/`emit_command_feedback` are `pub(crate)`).
  - [ ] 7.9 Guard: confirm `mmd_engine::rts::weapon` resolves (T3's re-export). If T3 exported it only as `mmd_engine::rts::combat::weapon` or from another module, fix the import paths in 1.6, 5.4 and 7.2 — never re-implement the fn.
- [ ] 8. **Tests (red first, per the Test plan tables above)**
  - [ ] 8.1 New `crates/mmd-engine/tests/rts_combat_commands.rs`: the 9 command tests. Local helpers: `fn scene() -> RtsHarness` (`RtsHarness::scene().build().expect(...)` when no enemy needed), `fn spawn_soldier(h, pos) -> EntityId` / `fn spawn_ghoul(h, pos) -> EntityId` wrapping `entities_mut().spawn(EntityKind::Unit(UnitKind::Soldier), OWNER_PLAYER, pos)` / `(UnitKind::Ghoul, OWNER_ENEMY, pos)` at hand-picked body-clear cells (copy coordinates style from `rts_hud.rs:585`); `fn select(h, ids)` inserting via `selection_mut()`.
  - [ ] 8.2 Append the 3 selection tests to `crates/mmd-engine/tests/rts_selection.rs` (use its existing view/projection helpers; enemy click point = centre of `sprite_screen_rect(view, ground)`).
  - [ ] 8.3 Append the 3 card/enemy-card tests to `crates/mmd-engine/tests/rts_hud.rs` (reuse its `scene()`/`workers()` helpers and the font-group text-scan helper already used by the detail-text tests).
  - [ ] 8.4 Append the 7 app tests to `src/rts_run.rs mod tests` — fixture world via `RtsWorld::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/scenarios/fixtures/fixture_rts_combat_v1.ron"))` or `tracked_world()` + `entities_mut()` spawns when no pre-placed enemy is needed; compute click points with `world.iso_view()` + `mmd_engine::rts::sprite_screen_rect`; assert through `session.audio_counters` and `world.order_of(...).unwrap().tag()`.
  - [ ] 8.5 Append the 2 feedback tests to `src/rts_feedback.rs mod tests` (its `receipt(i, order)` helper :755 exists).
  - [ ] 8.6 `cargo test --workspace --locked` — all new tests green, all old green (notably `rts_hud.rs worker_card_uses_stable_three_build_slots`, `mixed_or_empty_selection_disables_card`, the pinned `acceptance_audio_counts_are_exact` in `tests/rts_acceptance.rs`).

## Outputs

- Files: `crates/mmd-engine/src/rts/{world.rs, selection.rs, hud.rs, pack.rs, mod.rs}`, `src/{rts_run.rs, rts_ui.rs, rts_feedback.rs}`, `xtask/src/placeholder_art.rs`, regenerated `assets/sprites/generated/rts/{props.png, manifest.json}`, new `crates/mmd-engine/tests/rts_combat_commands.rs`, appended tests in `crates/mmd-engine/tests/{rts_selection.rs, rts_hud.rs}` + `src/{rts_run.rs, rts_feedback.rs}`.
- **Public API T7 consumes verbatim:**
  - `RtsWorld::cmd_attack_target(&mut self, target: EntityId, receipts: &mut OrderReceiptBuffer) -> CommandReceipt`
  - `RtsWorld::cmd_attack_move(&mut self, cell: Cell, receipts: &mut OrderReceiptBuffer) -> CommandReceipt`
  - `RtsWorld::cmd_stop(&mut self, receipts: &mut OrderReceiptBuffer) -> CommandReceipt`
  - `mmd_engine::rts::{CommandReceipt, CommandRejectReason}`
- **Script tokens T7 uses to fight (all pre-existing, none added):**
  - `FRAME:key:a` — positional slot 3 → `CommandId::Attack` → arms attack-targeting (armed selection required for the slot to be populated)
  - `FRAME:key:s` — positional slot 4 → `CommandId::Stop`
  - `FRAME:lclick:X,Y` — resolves an armed targeting mode (enemy sprite → `cmd_attack_target`, ground → `cmd_attack_move`)
  - `FRAME:rclick:X,Y` — enemy sprite + armed selection → `cmd_attack_target`; cancels an armed mode; ground semantics unchanged
  - `FRAME:key:esc` — cancels an armed mode (menu unchanged on that press)
  - `move`/`drag`/`sclick`/`quit` — unchanged
- Exit-line counters `voice_order` / `voice_reject` move on the new paths with no plumbing (D4).

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace --locked`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo run -p xtask -- atlases --check` → `atlases: ok (...)`
- [ ] `git diff --stat assets/scenarios/` → empty (tracked scenes + scripts untouched); `git status --porcelain assets/sprites/` shows only the two committed rts sheet files
- [ ] `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` → exit 0, `rts: clean exit` line with the same pinned `voice_select/voice_order/voice_reject/sfx_ui` values as before this ticket (scripts never select a soldier when pressing keys, no enemies in the gate scene)
- [ ] Manual (optional, windowed host): `cargo run -- rts` — box-select, produce a Soldier, press `A`, click ground → order voice; press `S` → order voice; `A` then right-click → silence (cancel); Escape after `A` → menu does **not** open
- [ ] commit msg draft: `feat(rts): attack, attack-move and stop as player commands`
