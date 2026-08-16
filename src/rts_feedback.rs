//! Deterministic semantic audio feedback for the `rts` command (T15).
//!
//! Every audio type lives here, in the app crate, on purpose: `RtsWorld`
//! stays clock-free, deterministic and audio-free, so nothing in this file
//! may ever reach the world state hash (ADR 020). The app derives *semantic*
//! events from the receipts the shared action path already returns, and a
//! [`AudioSink`] turns them into I/O — or, in every test and offscreen run,
//! into a fixed-capacity trace ([`FakeAudioSink`]) that never opens a
//! physical device.
//!
//! # Event semantics
//!
//! - **Selection** — snapshot the selected *player unit* ids before an
//!   action, snapshot again after, and voice the sorted set difference,
//!   capped at [`MAX_UNIT_CUES_PER_ACTION`]. Re-selecting an already
//!   selected unit, or deselecting, is silence. Buildings and nodes never
//!   speak.
//! - **Orders** — one cue per accepted [`UnitOrderReceipt`], merged and
//!   sorted globally across the whole action and capped at
//!   [`MAX_UNIT_CUES_PER_ACTION`]; accepted overflow past the cap is *not* a
//!   rejection. Any rejected candidate adds exactly one [`AudioEvent::Reject`]
//!   for the whole action, whatever the reason.
//! - **UI SFX** — one cue per successful, enabled pointer activation. A
//!   disabled control, a consumed background click, and every keyboard
//!   hotkey are silent.
//! - **Music** — one [`AudioEvent::StartMusic`] per session, before frame 1;
//!   nothing ever stops it, so it stays logically running through the pause
//!   menu and focus loss.
//!
//! Derivation is pure and shared: the live SDL path and the scripted path
//! both run it from `rts_run::apply`, so the two cannot drift.

use mmd_engine::rts::{
    ContextOrderResult, EntityId, EntityKind, IssuedOrder, OWNER_PLAYER, RtsWorld, UnitOrderReceipt,
};

use crate::rts_settings::AudioSettings;

/// Hard cap on unit voice cues per player action, shared by selection and
/// order batches — a global cap, not a per-kind one.
pub const MAX_UNIT_CUES_PER_ACTION: usize = 8;

/// Fixed [`FakeAudioSink`] trace capacity. Reserved once at construction;
/// the sink counts (never stores) anything past it.
pub const FAKE_TRACE_CAP: usize = 256;

/// One mixing bus. Master is a scalar over all three, not a bus of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioBus {
    Music,
    Voice,
    Sfx,
}

/// What a unit says when an action lands on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceCue {
    Select,
    Move,
    Gather,
    Build,
}

impl VoiceCue {
    /// The cue an accepted context order maps to.
    pub fn from_issued(order: IssuedOrder) -> Self {
        match order {
            IssuedOrder::Move => Self::Move,
            IssuedOrder::Gather => Self::Gather,
            IssuedOrder::Build => Self::Build,
        }
    }
}

/// One unit's cue in a batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitCue {
    pub entity: EntityId,
    pub cue: VoiceCue,
}

/// Up to [`MAX_UNIT_CUES_PER_ACTION`] cues for one player action, ascending
/// by entity id. Fixed size: building one allocates nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceBatch {
    pub entries: [Option<UnitCue>; MAX_UNIT_CUES_PER_ACTION],
    pub len: u8,
}

/// Sort key for a batch: slot index first, then generation, so a recycled
/// slot cannot reorder against its own predecessor.
fn id_key(id: EntityId) -> (u32, u32) {
    (id.index, id.generation)
}

impl Default for VoiceBatch {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceBatch {
    pub const fn new() -> Self {
        Self {
            entries: [None; MAX_UNIT_CUES_PER_ACTION],
            len: 0,
        }
    }

    /// Insert `cue` in ascending id order, dropping the largest id when the
    /// batch is already full — so a batch built from any order of the same
    /// ids is the same ascending first-[`MAX_UNIT_CUES_PER_ACTION`].
    pub fn push_sorted(&mut self, cue: UnitCue) {
        let len = self.len as usize;
        let mut pos = len;
        for i in 0..len {
            let existing = self.entries[i].expect("the first `len` entries are populated");
            if id_key(cue.entity) < id_key(existing.entity) {
                pos = i;
                break;
            }
        }
        if pos >= MAX_UNIT_CUES_PER_ACTION {
            // Larger than every cue already kept, and there is no room: this
            // one is the overflow, not one of the kept ones.
            return;
        }
        let end = len.min(MAX_UNIT_CUES_PER_ACTION - 1);
        for i in (pos..end).rev() {
            self.entries[i + 1] = self.entries[i];
        }
        self.entries[pos] = Some(cue);
        if len < MAX_UNIT_CUES_PER_ACTION {
            self.len += 1;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The batch's cues, ascending.
    pub fn cues(&self) -> impl Iterator<Item = UnitCue> + '_ {
        self.entries
            .iter()
            .take(self.len as usize)
            .filter_map(|e| *e)
    }
}

/// Which UI surface a click SFX came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiCue {
    /// Menu control, and every pause-menu navigation button.
    Menu,
    /// A settings control that accepted a new value.
    Settings,
    /// An enabled command-grid card.
    CommandGrid,
    /// A minimap click that actually recentred the camera.
    Minimap,
}

/// One semantic thing the game wants heard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioEvent {
    StartMusic,
    Voice(VoiceBatch),
    Reject,
    Ui(UiCue),
}

/// Per-bus gain in integer basis points (`master% * bus%`), so the value a
/// test asserts is exact. The SDL sink converts with `/ 10000.0` (T16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectiveGains {
    pub music_basis_points: u16,
    pub voice_basis_points: u16,
    pub sfx_basis_points: u16,
}

impl EffectiveGains {
    pub fn for_bus(&self, bus: AudioBus) -> u16 {
        match bus {
            AudioBus::Music => self.music_basis_points,
            AudioBus::Voice => self.voice_basis_points,
            AudioBus::Sfx => self.sfx_basis_points,
        }
    }
}

/// One bus product in basis points. Mute flags zero gain without touching
/// stored volume levels; unmuted path keeps integer `master * bus`.
fn bus_level(master: u32, bus: u32, master_muted: bool, bus_muted: bool) -> u16 {
    if master_muted || bus_muted {
        0
    } else {
        (master.min(100) * bus.min(100)) as u16
    }
}

/// `master% * bus%` in basis points, then mute mask. Levels already
/// validated to `0..=100` by [`crate::rts_settings::RtsSettings::validate`].
pub fn effective_gains(audio: &AudioSettings) -> EffectiveGains {
    EffectiveGains {
        music_basis_points: bus_level(
            audio.master,
            audio.music,
            audio.master_muted,
            audio.music_muted,
        ),
        voice_basis_points: bus_level(
            audio.master,
            audio.voice,
            audio.master_muted,
            audio.voice_muted,
        ),
        sfx_basis_points: bus_level(audio.master, audio.sfx, audio.master_muted, audio.sfx_muted),
    }
}

/// Why a sink could not do what it was asked. Opaque on purpose: T16's SDL
/// sink is the only thing that will ever produce a non-test value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioError(pub String);

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The only thing that turns semantic events into sound. Everything above
/// this trait is deterministic; everything below it is I/O.
pub trait AudioSink {
    fn set_gains(&mut self, gains: EffectiveGains) -> Result<(), AudioError>;
    fn emit(&mut self, event: AudioEvent) -> Result<(), AudioError>;
    /// Per-rendered-frame upkeep (music refill in T16). Never emits.
    fn maintain(&mut self) -> Result<(), AudioError>;
}

/// What a run heard, as plain counters — the observation seam the exit-time
/// `rts: audio` line and [`FakeAudioSink`] both report from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioCounters {
    pub music: u32,
    /// Voice *batches* (one per action that voiced anything).
    pub voice: u32,
    /// Individual unit cues inside those batches.
    pub cues: u32,
    /// Of [`Self::cues`], the [`VoiceCue::Select`] ones — what the run's
    /// selections voiced (`T17`'s `voice_select` exit field).
    pub select_cues: u32,
    /// Of [`Self::cues`], the accepted-order ones (`Move`/`Gather`/`Build`)
    /// — `T17`'s `voice_order` exit field.
    pub order_cues: u32,
    pub reject: u32,
    pub ui: u32,
}

impl AudioCounters {
    pub fn record(&mut self, event: &AudioEvent) {
        match event {
            AudioEvent::StartMusic => self.music += 1,
            AudioEvent::Voice(batch) => {
                self.voice += 1;
                for cue in batch.cues() {
                    self.cues += 1;
                    match cue.cue {
                        VoiceCue::Select => self.select_cues += 1,
                        VoiceCue::Move | VoiceCue::Gather | VoiceCue::Build => self.order_cues += 1,
                    }
                }
            }
            AudioEvent::Reject => self.reject += 1,
            AudioEvent::Ui(_) => self.ui += 1,
        }
    }
}

/// Fixed-capacity recording sink. Used by every test and by every offscreen
/// run: it never opens a device, and it never grows its trace after
/// [`Self::new`].
pub struct FakeAudioSink {
    trace: Vec<AudioEvent>,
    counters: AudioCounters,
    gains: EffectiveGains,
    gain_calls: u32,
    dropped: u32,
    fail_set_gains: bool,
}

impl Default for FakeAudioSink {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeAudioSink {
    pub fn new() -> Self {
        Self {
            trace: Vec::with_capacity(FAKE_TRACE_CAP),
            counters: AudioCounters::default(),
            gains: EffectiveGains {
                music_basis_points: 0,
                voice_basis_points: 0,
                sfx_basis_points: 0,
            },
            gain_calls: 0,
            dropped: 0,
            fail_set_gains: false,
        }
    }
}

// Read/inject-only accessors: the app itself only ever writes through the
// `AudioSink` impl, so these are test observation seams.
#[cfg_attr(not(test), allow(dead_code))]
impl FakeAudioSink {
    pub fn trace(&self) -> &[AudioEvent] {
        &self.trace
    }

    pub fn counters(&self) -> AudioCounters {
        self.counters
    }

    pub fn gains(&self) -> EffectiveGains {
        self.gains
    }

    pub fn gain_calls(&self) -> u32 {
        self.gain_calls
    }

    /// Events past [`FAKE_TRACE_CAP`]: counted, never stored.
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    pub fn capacity(&self) -> usize {
        self.trace.capacity()
    }

    /// Fault injection for the T13 settings rollback path.
    pub fn set_fail_set_gains(&mut self, fail: bool) {
        self.fail_set_gains = fail;
    }

    /// Every `Ui` cue in trace order.
    pub fn ui_cues(&self) -> Vec<UiCue> {
        self.trace
            .iter()
            .filter_map(|e| match e {
                AudioEvent::Ui(cue) => Some(*cue),
                _ => None,
            })
            .collect()
    }
}

impl AudioSink for FakeAudioSink {
    fn set_gains(&mut self, gains: EffectiveGains) -> Result<(), AudioError> {
        self.gain_calls += 1;
        if self.fail_set_gains {
            return Err(AudioError("injected set_gains failure".to_string()));
        }
        self.gains = gains;
        Ok(())
    }

    fn emit(&mut self, event: AudioEvent) -> Result<(), AudioError> {
        self.counters.record(&event);
        if self.trace.len() < FAKE_TRACE_CAP {
            self.trace.push(event);
        } else {
            self.dropped += 1;
        }
        Ok(())
    }

    fn maintain(&mut self) -> Result<(), AudioError> {
        Ok(())
    }
}

/// A shared handle on one [`FakeAudioSink`], so a test can keep observing
/// the sink a session took ownership of.
#[cfg(test)]
#[derive(Clone)]
pub struct FakeSinkHandle(std::rc::Rc<std::cell::RefCell<FakeAudioSink>>);

#[cfg(test)]
impl FakeSinkHandle {
    pub fn new() -> Self {
        Self(std::rc::Rc::new(std::cell::RefCell::new(
            FakeAudioSink::new(),
        )))
    }

    /// The sink itself, for assertions.
    pub fn sink(&self) -> std::cell::Ref<'_, FakeAudioSink> {
        self.0.borrow()
    }

    /// Fault-inject `set_gains` failures through the shared sink (`T3`).
    pub fn set_fail_set_gains(&self, fail: bool) {
        self.0.borrow_mut().set_fail_set_gains(fail);
    }

    /// Another handle on the same sink, as a boxed [`AudioSink`].
    pub fn boxed(&self) -> Box<dyn AudioSink> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
impl AudioSink for FakeSinkHandle {
    fn set_gains(&mut self, gains: EffectiveGains) -> Result<(), AudioError> {
        self.0.borrow_mut().set_gains(gains)
    }
    fn emit(&mut self, event: AudioEvent) -> Result<(), AudioError> {
        self.0.borrow_mut().emit(event)
    }
    fn maintain(&mut self) -> Result<(), AudioError> {
        self.0.borrow_mut().maintain()
    }
}

/// A sink that hears nothing — the control case proving audio derivation
/// cannot reach world state.
#[cfg_attr(not(test), allow(dead_code))]
pub struct NullAudioSink;

impl AudioSink for NullAudioSink {
    fn set_gains(&mut self, _gains: EffectiveGains) -> Result<(), AudioError> {
        Ok(())
    }
    fn emit(&mut self, _event: AudioEvent) -> Result<(), AudioError> {
        Ok(())
    }
    fn maintain(&mut self) -> Result<(), AudioError> {
        Ok(())
    }
}

/// The currently selected **player units**, ascending, into `out`.
///
/// Buildings and nodes are filtered out here: they never voice, so they can
/// never enter a selection delta. `out` is cleared, never grown past the
/// selection's own cap, so a snapshot allocates nothing after the first.
pub fn snapshot_selected_units(world: &RtsWorld, out: &mut Vec<EntityId>) {
    out.clear();
    let store = world.entities();
    for &id in world.selection().ids() {
        let Some(slot) = store.slot(id) else { continue };
        if store.owner(slot) == OWNER_PLAYER && matches!(store.kind(slot), EntityKind::Unit(_)) {
            out.push(id);
        }
    }
}

/// The Select batch for one action: every id in `after` that was not in
/// `before`, ascending, first [`MAX_UNIT_CUES_PER_ACTION`].
///
/// `None` when nothing new was selected — a re-selection or a deselection is
/// silence.
pub fn selection_cues(before: &[EntityId], after: &[EntityId]) -> Option<VoiceBatch> {
    let mut batch = VoiceBatch::new();
    for &id in after {
        if !before.contains(&id) {
            batch.push_sorted(UnitCue {
                entity: id,
                cue: VoiceCue::Select,
            });
        }
    }
    (!batch.is_empty()).then_some(batch)
}

/// The order batch for one action: one cue per accepted receipt, merged and
/// sorted across every order kind, first [`MAX_UNIT_CUES_PER_ACTION`].
///
/// `None` when nothing was accepted.
pub fn order_cues(receipts: &[UnitOrderReceipt]) -> Option<VoiceBatch> {
    let mut batch = VoiceBatch::new();
    for receipt in receipts {
        batch.push_sorted(UnitCue {
            entity: receipt.id,
            cue: VoiceCue::from_issued(receipt.order),
        });
    }
    (!batch.is_empty()).then_some(batch)
}

/// The single reject cue for one action, if any candidate was refused.
///
/// One per *action*, never per unit, and never for accepted overflow past
/// [`MAX_UNIT_CUES_PER_ACTION`] — that is a cap, not a rejection.
pub fn reject_cue(result: &ContextOrderResult) -> Option<AudioEvent> {
    (result.rejected > 0).then_some(AudioEvent::Reject)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rts_input::RtsCommand;
    use crate::rts_run::{RtsSession, apply};
    use crate::rts_settings::RtsSettings;
    use mmd_engine::rts::{CommandId, HudHit, ModalHit, UnitKind, command_slots};
    use mmd_engine::rts::{HudLayout, minimap_projection};

    fn id(index: u32) -> EntityId {
        EntityId {
            index,
            generation: 1,
        }
    }

    fn receipt(index: u32, order: IssuedOrder) -> UnitOrderReceipt {
        UnitOrderReceipt {
            id: id(index),
            order,
        }
    }

    fn result(accepted: usize, rejected: usize) -> ContextOrderResult {
        ContextOrderResult {
            pick: mmd_engine::rts::Pick::Nothing,
            accepted,
            rejected,
            reason: None,
        }
    }

    /// Not `testkit::RtsHarness` (feature-gated out of this binary crate) —
    /// the tracked scenario, loaded exactly as `rts_run::run` loads it.
    fn test_world() -> RtsWorld {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/scenarios/rts_prototype_v1.ron");
        RtsWorld::load(&path).expect("tracked scenario loads")
    }

    fn player_units(world: &RtsWorld, kind: UnitKind) -> Vec<EntityId> {
        let store = world.entities();
        (0..store.slot_count())
            .filter(|&slot| {
                store.alive(slot)
                    && store.owner(slot) == OWNER_PLAYER
                    && store.kind(slot) == EntityKind::Unit(kind)
            })
            .filter_map(|slot| store.id_at(slot))
            .collect()
    }

    fn first_building(world: &RtsWorld) -> EntityId {
        let store = world.entities();
        (0..store.slot_count())
            .filter(|&slot| {
                store.alive(slot) && matches!(store.kind(slot), EntityKind::Building(_))
            })
            .find_map(|slot| store.id_at(slot))
            .expect("the tracked scenario seeds an HQ")
    }

    // -- Gains -----------------------------------------------------------

    #[test]
    fn default_effective_gains_are_exact() {
        let audio = RtsSettings::default().audio;
        assert_eq!(
            (audio.master, audio.music, audio.voice, audio.sfx),
            (80, 35, 70, 60)
        );

        let gains = effective_gains(&audio);

        assert_eq!(gains.music_basis_points, 2800);
        assert_eq!(gains.voice_basis_points, 5600);
        assert_eq!(gains.sfx_basis_points, 4800);
        assert_eq!(gains.for_bus(AudioBus::Music), 2800);
        assert_eq!(gains.for_bus(AudioBus::Voice), 5600);
        assert_eq!(gains.for_bus(AudioBus::Sfx), 4800);
    }

    #[test]
    fn muted_master_zeroes_every_bus() {
        let audio = AudioSettings {
            master: 0,
            ..RtsSettings::default().audio
        };
        let gains = effective_gains(&audio);
        assert_eq!(
            (
                gains.music_basis_points,
                gains.voice_basis_points,
                gains.sfx_basis_points
            ),
            (0, 0, 0)
        );
    }

    #[test]
    fn master_mute_zeroes_all_buses_without_changing_levels() {
        let mut audio = AudioSettings {
            master: 80,
            music: 35,
            voice: 70,
            sfx: 60,
            ..AudioSettings::default()
        };
        audio.master_muted = true;

        let gains = effective_gains(&audio);
        assert_eq!(
            (
                gains.music_basis_points,
                gains.voice_basis_points,
                gains.sfx_basis_points
            ),
            (0, 0, 0)
        );
        assert_eq!(
            (audio.master, audio.music, audio.voice, audio.sfx),
            (80, 35, 70, 60)
        );

        audio.master_muted = false;
        let restored = effective_gains(&audio);
        assert_eq!(
            (
                restored.music_basis_points,
                restored.voice_basis_points,
                restored.sfx_basis_points
            ),
            (2800, 5600, 4800)
        );
    }

    #[test]
    fn bus_mute_zeroes_only_its_bus() {
        let mut audio = AudioSettings {
            master: 80,
            music: 35,
            voice: 70,
            sfx: 60,
            ..AudioSettings::default()
        };
        audio.music_muted = true;

        let gains = effective_gains(&audio);
        assert_eq!(gains.music_basis_points, 0);
        assert_eq!(gains.voice_basis_points, 5600);
        assert_eq!(gains.sfx_basis_points, 4800);
        assert_eq!(audio.music, 35, "level stays while muted");

        audio.music_muted = false;
        assert_eq!(effective_gains(&audio).music_basis_points, 2800);
    }

    // -- Selection delta -------------------------------------------------

    #[test]
    fn selection_cues_only_new_player_units() {
        let mut world = test_world();
        let workers = player_units(&world, UnitKind::Worker);
        assert!(workers.len() >= 2, "scenario seeds several workers");
        let mut before = Vec::new();
        let mut after = Vec::new();

        // First selection: one cue for the newly selected worker.
        snapshot_selected_units(&world, &mut before);
        world.select_only(workers[0]);
        snapshot_selected_units(&world, &mut after);
        let batch = selection_cues(&before, &after).expect("a new selection voices");
        assert_eq!(batch.len, 1);
        assert_eq!(batch.cues().next().unwrap().entity, workers[0]);
        assert_eq!(batch.cues().next().unwrap().cue, VoiceCue::Select);

        // Re-selecting the same unit is silence.
        snapshot_selected_units(&world, &mut before);
        world.select_only(workers[0]);
        snapshot_selected_units(&world, &mut after);
        assert_eq!(selection_cues(&before, &after), None);

        // Deselecting is silence.
        snapshot_selected_units(&world, &mut before);
        world.toggle_selection(workers[0]);
        snapshot_selected_units(&world, &mut after);
        assert_eq!(selection_cues(&before, &after), None);

        // A building never voices: selecting one leaves the unit snapshot
        // empty, so there is no delta at all.
        snapshot_selected_units(&world, &mut before);
        world.select_only(first_building(&world));
        snapshot_selected_units(&world, &mut after);
        assert!(after.is_empty(), "buildings never enter a voice snapshot");
        assert_eq!(selection_cues(&before, &after), None);
    }

    #[test]
    fn selection_batch_is_sorted_and_capped() {
        let shuffled: Vec<EntityId> = [7, 3, 11, 1, 9, 5, 12, 2, 8, 4, 10, 6]
            .into_iter()
            .map(id)
            .collect();

        let batch = selection_cues(&[], &shuffled).expect("twelve new units voice");

        assert_eq!(batch.len as usize, MAX_UNIT_CUES_PER_ACTION);
        let ids: Vec<u32> = batch.cues().map(|c| c.entity.index).collect();
        assert_eq!(ids, vec![1, 2, 3, 4, 5, 6, 7, 8], "first eight, ascending");
        assert!(batch.cues().all(|c| c.cue == VoiceCue::Select));
    }

    // -- Order receipts --------------------------------------------------

    #[test]
    fn mixed_resource_order_uses_one_global_cap() {
        // Six workers gathering and six soldiers moving, interleaved: the
        // cap is global across both kinds, not per kind.
        let receipts: Vec<UnitOrderReceipt> = (0..12)
            .map(|i| {
                receipt(
                    12 - i,
                    if i % 2 == 0 {
                        IssuedOrder::Gather
                    } else {
                        IssuedOrder::Move
                    },
                )
            })
            .collect();

        let batch = order_cues(&receipts).expect("accepted orders voice");

        assert_eq!(batch.len as usize, MAX_UNIT_CUES_PER_ACTION);
        let ids: Vec<u32> = batch.cues().map(|c| c.entity.index).collect();
        assert_eq!(ids, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(
            batch.cues().any(|c| c.cue == VoiceCue::Gather)
                && batch.cues().any(|c| c.cue == VoiceCue::Move),
            "one capped batch mixes both cue kinds"
        );
    }

    #[test]
    fn partial_success_emits_voice_and_one_reject() {
        let receipts = [
            receipt(2, IssuedOrder::Build),
            receipt(1, IssuedOrder::Build),
        ];

        let batch = order_cues(&receipts).expect("the accepted half voices");
        let reject = reject_cue(&result(2, 3));

        assert_eq!(batch.len, 2);
        assert_eq!(
            batch.cues().map(|c| c.entity.index).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(reject, Some(AudioEvent::Reject), "exactly one reject cue");
    }

    #[test]
    fn total_rejection_emits_one_reject() {
        assert_eq!(order_cues(&[]), None, "nothing accepted, nothing voiced");
        assert_eq!(reject_cue(&result(0, 4)), Some(AudioEvent::Reject));
        // An empty selection resolves no candidates at all: silence.
        assert_eq!(reject_cue(&result(0, 0)), None);
    }

    #[test]
    fn accepted_overflow_is_not_rejection() {
        let receipts: Vec<UnitOrderReceipt> =
            (1..=20).map(|i| receipt(i, IssuedOrder::Move)).collect();

        let batch = order_cues(&receipts).expect("twenty accepted orders voice");

        assert_eq!(batch.len as usize, MAX_UNIT_CUES_PER_ACTION);
        assert_eq!(reject_cue(&result(20, 0)), None, "a cap is not a rejection");
    }

    // -- UI SFX ----------------------------------------------------------

    fn session_with_world() -> (RtsWorld, RtsSession, FakeSinkHandle) {
        let (session, handle) = RtsSession::for_test();
        (test_world(), session, handle)
    }

    #[test]
    fn ui_actions_map_to_sfx_sources() {
        let (mut world, mut session, audio) = session_with_world();
        let worker = player_units(&world, UnitKind::Worker)[0];
        world.select_only(worker);

        // 1. Menu control -> Menu cue.
        crate::rts_ui::handle_hud_click(&mut world, &mut session, HudHit::Menu, false);
        // 2. Pause menu's Settings button -> Menu, then a settings edit ->
        //    Settings.
        crate::rts_ui::handle_modal_click(&mut session, ModalHit::OpenSettings);
        crate::rts_ui::handle_modal_click(&mut session, ModalHit::Master(55));
        // 3. An enabled command card -> CommandGrid.
        let slot = command_slots(&world)
            .iter()
            .position(|s| s.enabled && s.command == Some(CommandId::BuildHq))
            .expect("a selected worker enables BuildHq");
        crate::rts_ui::handle_hud_click(
            &mut world,
            &mut session,
            HudHit::CommandSlot(slot as u8),
            false,
        );
        // 4. A minimap click inside the map diamond -> Minimap.
        let origin = [HudLayout::MINIMAP_MAP[0], HudLayout::MINIMAP_MAP[1]];
        let projection = minimap_projection(&world);
        let center = [
            origin[0] + HudLayout::MINIMAP_MAP[2] * 0.5,
            origin[1] + HudLayout::MINIMAP_MAP[3] * 0.5,
        ];
        assert!(
            projection
                .minimap_to_map([center[0] - origin[0], center[1] - origin[1]])
                .is_some(),
            "the diamond centre is inside the map"
        );
        crate::rts_ui::handle_hud_click(&mut world, &mut session, HudHit::Minimap(center), false);

        assert_eq!(
            audio.sink().ui_cues(),
            vec![
                UiCue::Menu,
                UiCue::Menu,
                UiCue::Settings,
                UiCue::CommandGrid,
                UiCue::Minimap
            ],
        );
    }

    #[test]
    fn disabled_or_keyboard_action_has_no_ui_sfx() {
        let (mut world, mut session, audio) = session_with_world();
        // Nothing selected: every command slot is disabled.
        assert!(
            command_slots(&world).iter().all(|s| !s.enabled),
            "an empty selection disables the whole grid"
        );
        crate::rts_ui::handle_hud_click(&mut world, &mut session, HudHit::CommandSlot(0), false);
        // HUD background: consumed, silent.
        crate::rts_ui::handle_hud_click(&mut world, &mut session, HudHit::Background, false);
        // A minimap click outside the map diamond: consumed, silent.
        let corner = [
            HudLayout::MINIMAP_MAP[0] + 1.0,
            HudLayout::MINIMAP_MAP[1] + 1.0,
        ];
        crate::rts_ui::handle_hud_click(&mut world, &mut session, HudHit::Minimap(corner), false);
        // The keyboard hotkey path never claims a pointer click.
        apply(&mut world, &mut session, RtsCommand::ExecuteSlot(0));

        assert_eq!(audio.sink().ui_cues(), Vec::<UiCue>::new());
    }

    #[test]
    fn activation_requires_matching_down_and_up_control() {
        let (mut world, mut session, audio) = session_with_world();
        session.ui.open_menu();
        let settings = [
            HudLayout::PAUSE_MENU_SETTINGS_BTN[0] + 10.0,
            HudLayout::PAUSE_MENU_SETTINGS_BTN[1] + 10.0,
        ];
        let close = [
            HudLayout::PAUSE_MENU_CLOSE_BTN[0] + 10.0,
            HudLayout::PAUSE_MENU_CLOSE_BTN[1] + 10.0,
        ];
        // Down Settings, up Close → no activation / no cue.
        crate::rts_run::pointer_down(&world, &mut session, settings);
        crate::rts_run::pointer_up(&mut world, &mut session, close, false);
        assert_eq!(session.ui.page, crate::rts_ui::UiPage::PauseMenu);
        assert!(audio.sink().ui_cues().is_empty());

        // Matching down/up on Settings still opens.
        crate::rts_run::pointer_down(&world, &mut session, settings);
        crate::rts_run::pointer_up(&mut world, &mut session, settings, false);
        assert_eq!(session.ui.page, crate::rts_ui::UiPage::Settings);
        assert_eq!(audio.sink().ui_cues(), vec![UiCue::Menu]);
    }

    #[test]
    fn modal_press_never_leaks_to_world() {
        let (mut world, mut session, _audio) = session_with_world();
        let before = world.selection().ids().to_vec();
        session.ui.open_menu();
        let modal_gap = [10.0, 10.0];
        let world_pt = [960.0, 400.0];
        // Down on modal, move to world, up — no select/order.
        crate::rts_run::pointer_down(&world, &mut session, modal_gap);
        crate::rts_run::pointer_up(&mut world, &mut session, world_pt, false);
        assert_eq!(world.selection().ids(), before.as_slice());
        assert_eq!(session.ui.page, crate::rts_ui::UiPage::PauseMenu);
    }

    // -- Music -----------------------------------------------------------

    #[test]
    fn music_starts_once_and_survives_focus() {
        let (mut world, mut session, audio) = session_with_world();

        session.start_music();
        // Menu, focus loss, and a second start request must not restart it,
        // and nothing ever stops it.
        apply(&mut world, &mut session, RtsCommand::Escape);
        session.ui.focus_lost();
        apply(&mut world, &mut session, RtsCommand::Escape);
        session.start_music();

        assert_eq!(audio.sink().counters().music, 1);
        assert_eq!(
            audio
                .sink()
                .trace()
                .iter()
                .filter(|e| **e == AudioEvent::StartMusic)
                .count(),
            1,
        );
    }

    // -- Isolation from world state --------------------------------------

    #[test]
    fn audio_events_do_not_change_world_hash() {
        let script = |world: &mut RtsWorld, session: &mut RtsSession| {
            let workers = player_units(world, UnitKind::Worker);
            session.start_music();
            world.select_only(workers[0]);
            apply(world, session, RtsCommand::LeftClick([960.0, 540.0]));
            apply(world, session, RtsCommand::RightClick([980.0, 560.0]));
            apply(world, session, RtsCommand::Escape);
            apply(world, session, RtsCommand::Escape);
            for _ in 0..30 {
                world.tick();
            }
        };

        let mut loud_world = test_world();
        let (mut loud, loud_audio) = RtsSession::for_test();
        script(&mut loud_world, &mut loud);

        let mut quiet_world = test_world();
        let mut quiet = RtsSession::with_sink(Box::new(NullAudioSink));
        script(&mut quiet_world, &mut quiet);

        assert!(
            loud_audio.sink().counters() != AudioCounters::default(),
            "the loud run must actually have emitted something"
        );
        assert_eq!(
            loud_world.state_hash(),
            quiet_world.state_hash(),
            "audio derivation must never reach world state"
        );
    }

    // -- Fake sink discipline --------------------------------------------

    #[test]
    fn fake_sink_never_grows_after_new() {
        let mut sink = FakeAudioSink::new();
        let capacity = sink.capacity();
        assert_eq!(capacity, FAKE_TRACE_CAP);

        for _ in 0..(FAKE_TRACE_CAP + 16) {
            sink.emit(AudioEvent::Reject).expect("the fake never fails");
        }

        assert_eq!(sink.capacity(), capacity, "the trace must never grow");
        assert_eq!(sink.trace().len(), FAKE_TRACE_CAP);
        assert_eq!(sink.dropped(), 16, "overflow is counted, not stored");
        assert_eq!(
            sink.counters().reject,
            (FAKE_TRACE_CAP + 16) as u32,
            "counters see every event, even the dropped ones"
        );
    }
}
