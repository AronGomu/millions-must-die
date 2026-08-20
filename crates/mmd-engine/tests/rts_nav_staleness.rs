//! A cached flow field is a *snapshot*: the pool rebuilds it, evicts it, or
//! invalidates it under the order that is riding it. These cases pin the rule
//! that an order notices and re-paths, rather than walking a field that no
//! longer answers the question it asked.
//!
//! Pure logic, no GPU, no clock: every case here is a headless CPU check of
//! `mmd_engine::rts` and `testkit::RtsHarness`.

use mmd_engine::nav::field_pool::NAV_FIELD_SLOTS;
use mmd_engine::rts::{
    BuildingKind, EntityId, EntityKind, FORMATION_ARRIVAL_CELLS, OWNER_PLAYER, Order,
    RTS_UNIT_BODY_RADIUS_CELLS, ResourceKind, UnitKind, units_overlap,
};
use mmd_engine::scenario::Cell;
use mmd_engine::testkit::{RtsHarness, fixture_path};

/// The enemy-free twin of the tracked scene, for horizons that cross its
/// first wave tick (3000): `fixture_rts_baseline_v1.ron` is a byte-identical
/// copy taken immediately before the combat gate scripted enemies into the
/// scene, so every pinned number below keeps its meaning.
fn baseline_scene() -> RtsHarness {
    RtsHarness::path(fixture_path("fixture_rts_baseline_v1"))
        .build()
        .expect("baseline scene harness")
}

/// The scene is 320 x 320 cells.
const GRID: u32 = 320;

/// Depot footprint min corner, clear of terrain, nodes and the HQ.
const DEPOT_MIN: Cell = Cell { x: 144, y: 176 };
/// The centre of that footprint (edge 8), where a caught unit ends up.
const DEPOT_CENTER: [f32; 2] = [148.0, 180.0];

fn spawn_worker(h: &mut RtsHarness, pos: [f32; 2]) -> EntityId {
    h.world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, pos)
        .expect("spawn a worker")
}

fn position_of(h: &RtsHarness, id: EntityId) -> [f32; 2] {
    let slot = h.world().entities().slot(id).expect("live entity");
    h.world().entities().position(slot)
}

fn cell_of(h: &RtsHarness, id: EntityId) -> Cell {
    let p = position_of(h, id);
    Cell {
        x: p[0].floor() as u32,
        y: p[1].floor() as u32,
    }
}

fn is_blocked(h: &RtsHarness, cell: Cell) -> bool {
    h.world().nav().blocked()[(cell.x + cell.y * GRID) as usize]
}

fn crystal_node(h: &RtsHarness) -> EntityId {
    h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0]
}

/// Place a Depot at [`DEPOT_MIN`], attended by a worker standing beside it.
/// Returns the site and the builder attending it.
///
/// The builder stands squarely off the footprint's east edge, not off its
/// south-east corner: since T4 a unit is a 3-cell body, and the corner spot
/// this used to use is a pocket — legal to stand in by 0.04 cells, with the
/// finished Depot's clearance on two sides — so a body parked there can be
/// neither walked around nor shoved aside, and it wallled in whatever the case
/// was actually about.
///
/// The east-edge spot it uses instead is legal, but it is *inside* the
/// single-file corridor the finished Depot leaves (see
/// [`a_parked_body_plugs_the_single_file_depot_corridor`]), so a case that
/// walks another body south past the Depot has to get this scaffolding body
/// out of the way first.
fn place_depot(h: &mut RtsHarness) -> (EntityId, EntityId) {
    let builder = spawn_worker(h, [155.0, 180.0]);
    assert!(
        h.world_mut().begin_placement(BuildingKind::Depot),
        "the scene's starting crystal must cover a Depot"
    );
    let site = h
        .world_mut()
        .confirm_placement(DEPOT_MIN, builder)
        .expect("Depot placement refused");
    (site, builder)
}

/// Fill every pool slot with fields nobody else is using, so any field an
/// existing order cached has been evicted.
///
/// A decoy worker takes the orders: `order_move` acquires, and acquiring
/// `NAV_FIELD_SLOTS` fresh destinations evicts the lot.
fn evict_every_field(h: &mut RtsHarness) {
    // Fixed, hand-verified legal (unblocked in the tracked scene's
    // radius-inflated navigation mask) cells — not a diagonal `(40+i, 260+i)`
    // stride, which lands on a solid cell for roughly half of `i` in this
    // scene's terrain and would make `order_move` refuse the decoy order
    // outright instead of evicting a slot.
    const DESTS: [Cell; NAV_FIELD_SLOTS] = [
        Cell { x: 44, y: 264 },
        Cell { x: 47, y: 267 },
        Cell { x: 54, y: 274 },
        Cell { x: 57, y: 277 },
        Cell { x: 64, y: 284 },
        Cell { x: 67, y: 287 },
        Cell { x: 73, y: 293 },
        Cell { x: 76, y: 296 },
    ];
    let decoy = spawn_worker(h, [44.5, 264.5]);
    for dest in DESTS {
        assert!(
            h.world_mut().order_move(decoy, dest),
            "the decoy move order was refused"
        );
    }
}

fn arrived(p: [f32; 2], dest: Cell) -> bool {
    let dx = p[0] - (dest.x as f32 + 0.5);
    let dy = p[1] - (dest.y as f32 + 0.5);
    dx * dx + dy * dy <= FORMATION_ARRIVAL_CELLS * FORMATION_ARRIVAL_CELLS
}

// --- a building finishing under a live order ---------------------------------

/// A finished building invalidates every cached field. A unit already walking
/// one of them must re-path around the new wall, not grind into it.
#[test]
fn a_walking_unit_re_paths_when_a_building_blocks_its_route() {
    let mut h = baseline_scene();
    // Far enough that even at the tripled worker speed the walker is still
    // in flight when the Depot's build timer lands the stamp, not already
    // idle at its destination.
    // The scene's own starting workers stand around (162..175, 190..197).
    // Since T4 they are 3-cell bodies: they would decide where this walker
    // gets to long before its cached field did. Clear them — this case is
    // about a *field* going stale, not about a crowd.
    for w in h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)) {
        assert!(h.world_mut().entities_mut().despawn(w));
    }
    // Since T7 the clear Depot corner sits west of the enlarged HQ, so the
    // route the Depot must land across runs north-to-south down its own
    // column, not west along the old east-side lane.
    let walker = spawn_worker(&mut h, [148.5, 110.5]);
    let dest = Cell { x: 148, y: 210 };
    assert!(h.world_mut().order_move(walker, dest));

    // The Depot lands squarely across the route the walker is already on.
    let (depot, builder) = place_depot(&mut h);
    // Every field this run needs is now built; nothing but the stamp can
    // force another rebuild.
    let rebuilds_before_stamp = h.world().nav().rebuild_count();
    let mut finished_at = None;
    for _ in 0..1_000 {
        h.step_exact(1);
        if !h.world().is_site(depot) {
            finished_at = Some(h.tick_index());
            break;
        }
    }
    let finished_at = finished_at.expect("the Depot never finished");
    // The builder is scaffolding, and since T7 it stands in the single-file
    // gap between the Depot and the HQ (see
    // [`a_parked_body_plugs_the_single_file_depot_corridor`]). Clear it: this
    // case is about a *field* going stale, not about a body in the way.
    assert!(h.world_mut().entities_mut().despawn(builder));

    h.step_exact(4_000);

    let p = position_of(&h, walker);
    assert!(
        arrived(p, dest),
        "the walker stopped at {p:?}, not within {FORMATION_ARRIVAL_CELLS} of \
         {dest:?}: the Depot finished at tick {finished_at} and the order kept \
         riding the field cached before the stamp"
    );
    assert_eq!(
        h.world().order_of(walker),
        Some(Order::Idle),
        "the move order never completed"
    );
    assert!(
        h.world().nav().rebuild_count() > rebuilds_before_stamp,
        "no field was rebuilt across the stamp: {rebuilds_before_stamp} rebuilds \
         before it and the same after"
    );
}

// --- an evicted slot under a live order --------------------------------------

/// `Gather` used to fall through on a bottomed-out field and hang forever.
#[test]
fn an_evicted_field_does_not_hang_a_gather() {
    let mut h = baseline_scene();
    let worker = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[0];
    let node = crystal_node(&h);
    assert!(h.world_mut().order_gather(worker, node));

    evict_every_field(&mut h);
    let before = h.world().resources().crystal;

    h.step_exact(6_000);

    assert!(
        h.world().resources().crystal > before,
        "crystal is {} after 6000 ticks — the gather order rode an evicted \
         field and never banked a load",
        h.world().resources().crystal
    );
}

/// `Order::Build` acquires once and never refreshes its LRU stamp, so it is
/// the *preferred* eviction victim — and used to hang, resources sunk.
#[test]
fn an_evicted_field_does_not_hang_a_build() {
    let mut h = baseline_scene();
    let (depot, _) = place_depot(&mut h);
    evict_every_field(&mut h);

    h.step_exact(4_000);

    assert!(
        !h.world().is_site(depot),
        "the site never finished: its builder rode an evicted field"
    );
}

// --- a unit caught inside a finishing footprint ------------------------------

/// A unit is not an obstruction, so a building can finish on top of one. Since
/// T6 it is not shoved off a blocked cell after the fact — the completion is
/// planned around it, and it ends the tick on a **legal, collision-free** body
/// centre outside the finished footprint, still able to take and finish an
/// order.
///
/// The walk target is the original (200, 200). Getting there means walking
/// south past the finished Depot, down the single-file corridor
/// [`a_parked_body_plugs_the_single_file_depot_corridor`] pins — so the
/// scaffolding builder, which stands in that corridor, is despawned once the
/// body-placement assertions it participates in have run. It is scaffolding,
/// not this case's subject.
#[test]
fn a_unit_caught_in_a_finished_footprint_escapes() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let caught = spawn_worker(&mut h, DEPOT_CENTER);
    let (depot, builder) = place_depot(&mut h);

    for _ in 0..1_000 {
        h.step_exact(1);
        if !h.world().is_site(depot) {
            break;
        }
    }
    assert!(!h.world().is_site(depot), "the Depot never finished");

    let cell = cell_of(&h, caught);
    assert!(
        !is_blocked(&h, cell),
        "the caught worker is standing in blocked cell {cell:?}"
    );
    let p = position_of(&h, caught);
    assert!(
        h.world()
            .static_nav()
            .position_clear(p, RTS_UNIT_BODY_RADIUS_CELLS),
        "the caught worker was left at {p:?}, penetrating static geometry"
    );
    for other in h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)) {
        if other == caught {
            continue;
        }
        let q = position_of(&h, other);
        assert!(
            !units_overlap(p, RTS_UNIT_BODY_RADIUS_CELLS, q, RTS_UNIT_BODY_RADIUS_CELLS),
            "the evacuated body at {p:?} merged into the body at {q:?}"
        );
    }

    // Every body assertion above has run, including against the builder.
    // Clear it out of the single-file corridor the route south uses — and the
    // scene's own six starting workers with it, which since T7 sit squarely
    // across the route east at (162..175, 190..197). This case is about one
    // evacuated body taking an order, not about a crowd.
    assert!(h.world_mut().entities_mut().despawn(builder));
    for other in h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)) {
        if other != caught {
            assert!(h.world_mut().entities_mut().despawn(other));
        }
    }

    let dest = Cell { x: 200, y: 200 };
    assert!(
        h.world_mut().order_move(caught, dest),
        "order_move refused a unit that was inside the footprint"
    );
    h.step_exact(2_000);
    let p = position_of(&h, caught);
    assert!(
        arrived(p, dest),
        "the caught worker stopped at {p:?}, not within {FORMATION_ARRIVAL_CELLS} \
         of {dest:?}"
    );
    assert_eq!(
        h.world().order_of(caught),
        Some(Order::Idle),
        "order_move returned true but left an order that never completed"
    );
}

/// A finished Depot at [`DEPOT_MIN`] leaves a **single-file** corridor on its
/// east side, and one idle body parked in it blocks every other body for good.
///
/// The geometry: since T7 the Depot occupies `[144, 152) x [176, 184)` and the
/// 24-cell HQ occupies `[160, 184) x [160, 184)`, so between them the only
/// legal body centres are the three columns `x = 155, 156, 157`.
/// Two 3-cell-radius bodies six cells apart do not fit side by side in three
/// columns, so the corridor passes one body at a time.
///
/// What then makes the block permanent is the push rule, not the navigation
/// mask: `StaticNav::center_blocked` and `StaticNav::sweep_clear` agree here
/// (every centre-to-centre step in this region is swept clear, and the same
/// walk arrives when the corridor is empty — the second half of this case).
/// The mover's candidate is refused only by `body_sweep_hit`, and
/// `try_push_chain` cannot rescue it: the parked body's contact-normal push
/// target lands on a `center_blocked` cell inside the Depot's clearance, so
/// the whole chain is rejected. The mover then takes its south-east
/// deflection, and the field vector at the deflected cell points back
/// south-west, so it ping-pongs between two cells forever with a live order.
///
/// A regression of **this branch** — hard 3-cell bodies are new here, and
/// before them a point-sized unit walked straight past — owned by the
/// collision/push rule (T5/T6), not by the T3/T4 navigation mask. The fix,
/// deferred because it re-bases the tracked acceptance run: retry the push
/// chain along the mover's own heading when the contact-normal target is
/// statically illegal. See `docs/ADR/017`.
#[test]
fn a_parked_body_plugs_the_single_file_depot_corridor() {
    // (a) Corridor plugged by the builder that raised the Depot.
    let mut h = baseline_scene();
    let (depot, builder) = place_depot(&mut h);
    for _ in 0..1_000 {
        h.step_exact(1);
        if !h.world().is_site(depot) {
            break;
        }
    }
    assert!(!h.world().is_site(depot), "the Depot never finished");
    assert_eq!(
        position_of(&h, builder),
        [155.0, 180.0],
        "the builder must still be parked in the corridor"
    );

    // North of the corridor's mouth, and a destination immediately south of
    // it: the walk is through the gap or not at all.
    let w = spawn_worker(&mut h, [156.5, 170.5]);
    let dest = Cell { x: 156, y: 186 };
    assert!(h.world_mut().order_move(w, dest));
    h.step_exact(3_000);

    let p = position_of(&h, w);
    assert!(
        !arrived(p, dest),
        "the corridor passed a second body around the parked one at {p:?} \
         — the push rule changed; update this case and `docs/ADR/017`"
    );
    assert!(
        matches!(h.world().order_of(w), Some(Order::Move { .. })),
        "the blocked body should still be holding a live order at {p:?}"
    );
    assert!(
        h.world()
            .static_nav()
            .position_clear(p, RTS_UNIT_BODY_RADIUS_CELLS),
        "the blocked body at {p:?} is not even clear of static geometry — \
         this case is about a body in the way, not about the nav mask"
    );

    // (b) The identical walk, with the corridor clear, arrives. This is what
    // makes (a) a statement about bodies and not about the mask.
    let mut h = baseline_scene();
    let (depot, builder) = place_depot(&mut h);
    for _ in 0..1_000 {
        h.step_exact(1);
        if !h.world().is_site(depot) {
            break;
        }
    }
    assert!(!h.world().is_site(depot), "the Depot never finished");
    assert!(h.world_mut().entities_mut().despawn(builder));

    let w = spawn_worker(&mut h, [156.5, 170.5]);
    assert!(h.world_mut().order_move(w, dest));
    h.step_exact(3_000);

    let p = position_of(&h, w);
    assert!(
        arrived(p, dest),
        "with the corridor clear the same walk must reach {dest:?}, but the \
         body stopped at {p:?}"
    );
}

/// The same unit, given the order that used to stay live forever with nothing
/// moving.
#[test]
fn a_unit_caught_in_a_finished_footprint_can_still_gather() {
    let mut h = baseline_scene();
    let caught = spawn_worker(&mut h, DEPOT_CENTER);
    let (depot, _) = place_depot(&mut h);
    for _ in 0..1_000 {
        h.step_exact(1);
        if !h.world().is_site(depot) {
            break;
        }
    }
    assert!(!h.world().is_site(depot), "the Depot never finished");

    let node = crystal_node(&h);
    let before = h.world().resources().crystal;
    assert!(h.world_mut().order_gather(caught, node));

    h.step_exact(6_000);

    assert!(
        h.world().resources().crystal > before,
        "crystal is {} after 6000 ticks — the caught worker never delivered",
        h.world().resources().crystal
    );
}
