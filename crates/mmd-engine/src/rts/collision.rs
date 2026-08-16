//! Hard body collision between RTS unit bodies.
//!
//! The horde sim (`crate::sim`) resolves crowding with *soft separation*: a
//! repulsion sum bends a descent vector and two agents may overlap. RTS units
//! do not get that. A unit is a circle of `entity::RTS_UNIT_BODY_RADIUS_CELLS`
//! cells, and no completed [`super::world::RtsWorld::tick`] may leave two of
//! those circles merged.
//!
//! Both predicates here are pure and exact, and both treat **contact as
//! legal**: two bodies whose centres are exactly one summed-radius apart are
//! touching, not penetrating. Penetration is the strict `<` case, tested on
//! squared distances so nothing ever takes a square root.
//!
//! [`moving_circle_hits_point`] is the one that makes the invariant safe at
//! speed: testing only the candidate *endpoint* would let a fast unit step
//! straight over a body it should have hit — the classic tunneling bug — so a
//! candidate is tested as the whole swept segment it traverses.
//!
//! Both predicates stay **pure geometry**: they know nothing about orders,
//! owners or kinds. Which pairs the world is allowed to leave merged is a
//! policy question, and [`GatherCollisionState`] is where the answer is
//! recorded — one byte per unordered slot pair, so every gate in the movement
//! system asks the same table rather than re-deriving the rule.

use sha2::{Digest, Sha256};

use super::entity::{
    EntityId, EntityKind, EntityStore, MAX_ENTITIES, RTS_UNIT_BODY_DIAMETER_CELLS,
};

/// Pair byte meaning "these two bodies are an *active* gather pair right now":
/// their mutual dynamic collision is exempt for this tick.
///
/// Only mutual, and only dynamic. Static geometry, every other pair, and every
/// placement search stay hard.
pub const GATHER_PAIR_ACTIVE: u8 = u8::MAX;

/// How many gradual separation attempts a pair that has *stopped* gathering
/// gets before its exit is handed to the relocation fallback.
///
/// The exemption an active gather pair enjoys does not survive the order that
/// earned it. It is not revoked in one frame either — that is the visible
/// teleport this bound exists to avoid — so an exited pair separates over at
/// most this many completed attempts, one per tick, and is then relocated. A
/// pair byte in `1..=GATHER_SEPARATION_TICKS` counts those completed attempts,
/// which is why the bound is an exemption's whole lifetime and not a grace
/// period that can be renewed.
pub const GATHER_SEPARATION_TICKS: u8 = 12;

/// How far one separation attempt may move one body: half a cell.
///
/// Derived, not chosen. The worst case an exit has to clear is two concentric
/// bodies, which are a whole [`RTS_UNIT_BODY_DIAMETER_CELLS`] from contact, so
/// a bound of [`GATHER_SEPARATION_TICKS`] attempts is only honest if each one
/// may cover its share of that distance — and exactly its share, so the worst
/// case resolves on the last attempt rather than falling through to the
/// fallback.
pub const GATHER_SEPARATION_STEP_CELLS: f32 =
    RTS_UNIT_BODY_DIAMETER_CELLS / GATHER_SEPARATION_TICKS as f32;

/// Whether a pair byte exempts its pair from *mutual dynamic* collision.
///
/// Two meanings, one answer: [`GATHER_PAIR_ACTIVE`] provenance, and a bounded
/// exit transition. `0` — hard — is the only byte that does not exempt, which
/// is what makes "clear the byte" the single way an exemption ends.
pub(crate) fn pair_byte_exempts(state: u8) -> bool {
    state != 0
}

/// Whether a pair byte is a bounded exit transition rather than active gather
/// provenance or a hard pair.
pub(crate) fn pair_byte_is_transition(state: u8) -> bool {
    (1..=GATHER_SEPARATION_TICKS).contains(&state)
}

/// The separation axis of a pair whose centres coincide, so "directly away"
/// has no direction to offer.
///
/// Points from the **lower** slot toward the higher one: the lower-slot body
/// separates along `-normal`, the higher-slot body along `+normal`, whichever
/// of the two the tick rotation picks as the mover. Keyed by the canonical
/// triangular pair index, so it is the same axis on every attempt that pair
/// ever makes, and neighbouring pairs do not all pile onto one axis.
pub(crate) fn concentric_pair_normal(a: usize, b: usize) -> [f32; 2] {
    const NORMALS: [[f32; 2]; 4] = [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]];
    NORMALS[pair_index(a, b) % NORMALS.len()]
}

/// Length of the dense triangular pair table: one byte per unordered pair of
/// the [`MAX_ENTITIES`] slots, `2 048 * 2 047 / 2 == 2_096_128`.
const PAIR_COUNT: usize = MAX_ENTITIES * (MAX_ENTITIES - 1) / 2;

/// Canonical index of the unordered slot pair `{a, b}` in a dense row-major
/// upper triangle. `a == b` is not a pair and is never asked for.
fn pair_index(a: usize, b: usize) -> usize {
    debug_assert_ne!(a, b, "a slot is not paired with itself");
    debug_assert!(a < MAX_ENTITIES && b < MAX_ENTITIES, "slot out of range");
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    lo * MAX_ENTITIES - lo * (lo + 1) / 2 + (hi - lo - 1)
}

/// Per-pair gather-collision provenance, preallocated once per world.
///
/// Dense rather than a map on purpose: the movement system asks about a pair
/// inside its innermost loops, so a lookup has to be an index, not a hash, and
/// the table must never allocate mid-tick. Two megabytes bought once at load
/// is the price of that.
///
/// # Slot reuse
///
/// A pair byte is keyed by *slot*, and slots are recycled. [`Self::sync_slot`]
/// is what keeps a newcomer from inheriting the exemptions of the entity that
/// used to live in its slot: the generation the table last saw for a slot is
/// remembered, and a change clears that slot's whole row before anything reads
/// it.
#[derive(Debug)]
pub(crate) struct GatherCollisionState {
    pairs: Vec<u8>,
    generations: Vec<u32>,
}

impl GatherCollisionState {
    pub(crate) fn new() -> Self {
        Self {
            pairs: vec![0; PAIR_COUNT],
            generations: vec![0; MAX_ENTITIES],
        }
    }

    /// Note that `id` occupies its slot, clearing the slot's whole pair row
    /// when it is a different entity than the one the table last saw there.
    ///
    /// Cheap when nothing changed — one `u32` compare — so the movement system
    /// can call it for every live unit every tick, which is where it is called
    /// from: the head of the movement pass, before any gate or oracle reads a
    /// pair byte.
    ///
    /// That placement leaves exactly one window, and it is not reachable from
    /// the game: a *unit* slot has to be recycled and then read before the
    /// next tick syncs it. Nothing in a shipping build despawns a unit at all
    /// (the only despawn is a cancelled building site, whose slot never
    /// carried a pair byte — only unit pairs are ever written), so reaching it
    /// takes a raw `testkit` despawn/respawn followed by a `state_hash` or
    /// `body_overlap_count` call with no tick in between. Deterministic even
    /// then, and cleared by the next tick.
    pub(crate) fn sync_slot(&mut self, id: EntityId) {
        let slot = id.index as usize;
        if self.generations[slot] == id.generation {
            return;
        }
        self.generations[slot] = id.generation;
        for other in 0..MAX_ENTITIES {
            if other != slot {
                self.pairs[pair_index(slot, other)] = 0;
            }
        }
    }

    pub(crate) fn state(&self, a: usize, b: usize) -> u8 {
        self.pairs[pair_index(a, b)]
    }

    pub(crate) fn set_state(&mut self, a: usize, b: usize, value: u8) {
        self.pairs[pair_index(a, b)] = value;
    }

    /// Feed the live half of the table into a state digest.
    ///
    /// [`EntityStore::hash_into`] carries neither slot index nor generation, so
    /// this block supplies both rather than assuming the digest already
    /// separates two different slot assignments. The frame is:
    ///
    /// 1. a `u64` count of live unit slots — fixes the block's own length, so
    ///    it cannot be confused with a longer or shorter one;
    /// 2. `(index, generation)` for each of those slots, ascending — pins which
    ///    unit every pair position below refers to, so a slot reused by a new
    ///    entity changes the digest even when its pair bytes match;
    /// 3. one byte per unordered pair in strict `(i, j)` ascending order,
    ///    including `0` — every live pair is framed, never only the marked
    ///    ones.
    pub(crate) fn hash_into(&self, h: &mut Sha256, store: &EntityStore) {
        let mut units: Vec<usize> = Vec::with_capacity(store.len());
        for slot in 0..store.slot_count() {
            if store.alive(slot) && matches!(store.kind(slot), EntityKind::Unit(_)) {
                units.push(slot);
            }
        }
        let n = units.len();
        h.update((n as u64).to_le_bytes());
        for &slot in &units {
            let id = store.id_at(slot).expect("live");
            h.update(id.index.to_le_bytes());
            h.update(id.generation.to_le_bytes());
        }
        for i in 0..n {
            for j in (i + 1)..n {
                h.update([self.state(units[i], units[j])]);
            }
        }
    }
}

/// Whether two circular bodies penetrate.
///
/// `false` at exactly `ar + br` apart: touching is legal.
pub fn units_overlap(a: [f32; 2], ar: f32, b: [f32; 2], br: f32) -> bool {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let sum = ar + br;
    dx * dx + dy * dy < sum * sum
}

/// Whether a circle of `radius` sweeping the straight segment `from -> to`
/// ever penetrates the stationary circle of `other_radius` at `other`.
///
/// Exact: the swept region of a circle along a segment is the set of points
/// within `radius` of that segment, so the test is point-to-segment distance
/// against the summed radii — no sampling, no step size, and therefore no
/// tunneling however long the segment is.
pub fn moving_circle_hits_point(
    from: [f32; 2],
    to: [f32; 2],
    radius: f32,
    other: [f32; 2],
    other_radius: f32,
) -> bool {
    let sum = radius + other_radius;
    dist2_point_seg(other, from, to) < sum * sum
}

/// Squared distance from a point to a closed segment. A zero-length segment
/// degenerates to the point-to-point case.
fn dist2_point_seg(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let abx = b[0] - a[0];
    let aby = b[1] - a[1];
    let apx = p[0] - a[0];
    let apy = p[1] - a[1];
    let len2 = abx * abx + aby * aby;
    let t = if len2 > 0.0 {
        ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dx = apx - abx * t;
    let dy = apy - aby * t;
    dx * dx + dy * dy
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rts::entity::{OWNER_PLAYER, ResourceKind, UnitKind};

    /// A store holding `units` worker bodies in slots `0..units`, plus one
    /// crystal node behind them — the pair table must ignore the node.
    fn store_with(units: usize) -> EntityStore {
        let mut store = EntityStore::new();
        for i in 0..units {
            store
                .spawn(
                    EntityKind::Unit(UnitKind::Worker),
                    OWNER_PLAYER,
                    [i as f32, 0.0],
                )
                .expect("spawn worker");
        }
        store
            .spawn(
                EntityKind::Node(ResourceKind::Crystal),
                OWNER_PLAYER,
                [0.0, 9.0],
            )
            .expect("spawn node");
        store
    }

    fn digest(state: &GatherCollisionState, store: &EntityStore) -> [u8; 32] {
        let mut h = Sha256::new();
        state.hash_into(&mut h, store);
        h.finalize().into()
    }

    #[test]
    fn triangular_pair_index_is_unique_and_bounded() {
        // Every unordered pair of the whole slot space maps to its own index,
        // and the image is exactly `0..PAIR_COUNT` — so the dense table is
        // both large enough and not one byte larger than it has to be.
        let mut seen = vec![false; PAIR_COUNT];
        for a in 0..MAX_ENTITIES {
            for b in (a + 1)..MAX_ENTITIES {
                let idx = pair_index(a, b);
                assert!(idx < PAIR_COUNT, "pair ({a}, {b}) indexed out of range");
                assert_eq!(idx, pair_index(b, a), "pair ({a}, {b}) is not canonical");
                assert!(!seen[idx], "pair ({a}, {b}) collides at index {idx}");
                seen[idx] = true;
            }
        }
        assert!(
            seen.into_iter().all(|s| s),
            "the table has unreachable bytes"
        );
        assert_eq!(PAIR_COUNT, 2_096_128);
    }

    #[test]
    fn pair_hash_frame_is_length_prefixed_and_identity_pinned() {
        let store = store_with(3);
        let mut state = GatherCollisionState::new();
        state.set_state(0, 2, GATHER_PAIR_ACTIVE);

        // The exact documented frame, written out by hand: `u64` count of live
        // unit slots, then `(index, generation)` per slot, then the triangular
        // byte run in `(i, j)` order — including the `0` bytes.
        let mut want = Sha256::new();
        want.update(3u64.to_le_bytes());
        for slot in 0..3usize {
            let id = store.id_at(slot).expect("live");
            want.update(id.index.to_le_bytes());
            want.update(id.generation.to_le_bytes());
        }
        want.update([state.state(0, 1)]);
        want.update([state.state(0, 2)]);
        want.update([state.state(1, 2)]);
        let want: [u8; 32] = want.finalize().into();

        assert_eq!(digest(&state, &store), want);
    }

    #[test]
    fn slot_reuse_clears_pair_row() {
        let mut store = store_with(3);
        let mut state = GatherCollisionState::new();
        for slot in 0..3usize {
            state.sync_slot(store.id_at(slot).expect("live"));
        }
        state.set_state(0, 1, GATHER_PAIR_ACTIVE);
        state.set_state(0, 2, GATHER_PAIR_ACTIVE);
        state.set_state(1, 2, GATHER_PAIR_ACTIVE);

        assert!(store.despawn(store.id_at(0).expect("live")));
        let reused = store
            .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [0.0, 0.0])
            .expect("spawn into the freed slot");
        assert_eq!(reused.index, 0, "the free list is LIFO: slot 0 comes back");
        state.sync_slot(reused);

        assert_eq!(state.state(0, 1), 0, "the newcomer inherited an exemption");
        assert_eq!(state.state(0, 2), 0, "the newcomer inherited an exemption");
        assert_eq!(
            state.state(1, 2),
            GATHER_PAIR_ACTIVE,
            "clearing one slot's row must not touch anyone else's pair"
        );
    }

    #[test]
    fn slot_reuse_changes_the_digest_at_equal_pair_bytes() {
        let mut store = store_with(2);
        let mut state = GatherCollisionState::new();
        state.set_state(0, 1, GATHER_PAIR_ACTIVE);
        let before = digest(&state, &store);

        assert!(store.despawn(store.id_at(0).expect("live")));
        let reused = store
            .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [0.0, 0.0])
            .expect("spawn into the freed slot");
        state.sync_slot(reused);
        state.set_state(0, 1, GATHER_PAIR_ACTIVE);

        assert_eq!(
            state.state(0, 1),
            GATHER_PAIR_ACTIVE,
            "the two worlds must really carry the same pair byte"
        );
        assert_ne!(
            digest(&state, &store),
            before,
            "a slot reused by a new entity must move the digest even when its \
             pair bytes are identical"
        );
    }

    #[test]
    fn state_hash_frames_every_live_pair_byte() {
        let store = store_with(4);
        let base = GatherCollisionState::new();
        let clean = digest(&base, &store);

        let mut seen = vec![clean];
        for a in 0..4usize {
            for b in (a + 1)..4usize {
                let mut state = GatherCollisionState::new();
                state.set_state(a, b, GATHER_PAIR_ACTIVE);
                let d = digest(&state, &store);
                assert!(
                    !seen.contains(&d),
                    "pair ({a}, {b}) is not framed independently"
                );
                seen.push(d);
            }
        }
        assert_eq!(seen.len(), 1 + 6, "four units make six framed pairs");
    }

    #[test]
    fn the_separation_bound_covers_the_worst_case_exit_exactly() {
        // The worst exit is two concentric bodies, a whole diameter from
        // contact. The bound is only honest if the attempts add up to exactly
        // that: short and the worst case always falls through to the fallback,
        // long and an attempt could overshoot past contact.
        assert_eq!(GATHER_SEPARATION_STEP_CELLS, 0.5);
        assert_eq!(
            GATHER_SEPARATION_STEP_CELLS * GATHER_SEPARATION_TICKS as f32,
            RTS_UNIT_BODY_DIAMETER_CELLS
        );
    }

    #[test]
    fn transition_bytes_are_exactly_one_through_the_bound() {
        assert!(!pair_byte_is_transition(0));
        assert!(!pair_byte_is_transition(GATHER_PAIR_ACTIVE));
        for n in 1..=GATHER_SEPARATION_TICKS {
            assert!(pair_byte_is_transition(n), "{n} is a completed attempt");
        }
        assert!(!pair_byte_is_transition(GATHER_SEPARATION_TICKS + 1));

        // Every byte but `0` exempts, so clearing the byte is the one way an
        // exemption ends.
        assert!(!pair_byte_exempts(0));
        assert!(pair_byte_exempts(GATHER_PAIR_ACTIVE));
        assert!(pair_byte_exempts(1));
        assert!(pair_byte_exempts(GATHER_SEPARATION_TICKS));
    }

    #[test]
    fn the_concentric_normal_is_cardinal_stable_and_lower_slot_signed() {
        for a in 0..16usize {
            for b in (a + 1)..16usize {
                let n = concentric_pair_normal(a, b);
                assert_eq!(
                    n[0] * n[0] + n[1] * n[1],
                    1.0,
                    "pair ({a}, {b}) has a non-unit normal {n:?}"
                );
                assert!(
                    n[0] == 0.0 || n[1] == 0.0,
                    "pair ({a}, {b}) is not axis-aligned: {n:?}"
                );
                // Unordered, so "lower slot goes along -normal" is well
                // defined however the caller names the pair.
                assert_eq!(n, concentric_pair_normal(b, a));
            }
        }
        // All four axes are actually used: a cluster of exits does not pile
        // onto one line.
        let mut axes: Vec<[f32; 2]> = Vec::new();
        for b in 1..8usize {
            let n = concentric_pair_normal(0, b);
            if !axes.contains(&n) {
                axes.push(n);
            }
        }
        assert_eq!(
            axes.len(),
            4,
            "only {axes:?} of the four axes are reachable"
        );
    }

    #[test]
    fn contact_is_not_overlap_on_either_axis() {
        assert!(!units_overlap([0.0, 0.0], 3.0, [6.0, 0.0], 3.0));
        assert!(!units_overlap([0.0, 0.0], 3.0, [0.0, 6.0], 3.0));
        assert!(units_overlap([0.0, 0.0], 3.0, [5.9, 0.0], 3.0));
    }

    #[test]
    fn a_zero_length_sweep_is_the_static_test() {
        let p = [4.0, 0.0];
        assert_eq!(
            moving_circle_hits_point([0.0, 0.0], [0.0, 0.0], 3.0, p, 3.0),
            units_overlap([0.0, 0.0], 3.0, p, 3.0)
        );
    }

    #[test]
    fn a_long_sweep_cannot_tunnel() {
        // 1 000 cells in one step, straight through a body at the midpoint.
        assert!(moving_circle_hits_point(
            [0.0, 0.0],
            [1000.0, 0.0],
            3.0,
            [500.0, 0.0],
            3.0
        ));
        // ...and misses one that sits a summed radius off the line.
        assert!(!moving_circle_hits_point(
            [0.0, 0.0],
            [1000.0, 0.0],
            3.0,
            [500.0, 6.0],
            3.0
        ));
    }
}
