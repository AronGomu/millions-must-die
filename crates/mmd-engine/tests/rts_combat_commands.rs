//! T4: the player command surface over T3's combat.
//!
//! Tests `cmd_attack_target`, `cmd_attack_move`, `cmd_stop` on `RtsWorld`,
//! validating receipts, rejects, and order transitions.

use mmd_engine::rts::{
    CommandRejectReason, EntityId, EntityKind, IssuedOrder, OWNER_ENEMY, OWNER_PLAYER, Order,
    OrderReceiptBuffer, UnitKind,
};
use mmd_engine::testkit::RtsHarness;

fn scene() -> RtsHarness {
    RtsHarness::scene().build().expect("rts scene harness")
}

fn spawn_soldier(h: &mut RtsHarness, pos: [f32; 2]) -> EntityId {
    h.world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Soldier), OWNER_PLAYER, pos)
        .expect("spawn soldier")
}

fn spawn_worker(h: &mut RtsHarness, pos: [f32; 2]) -> EntityId {
    h.world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, pos)
        .expect("spawn worker")
}

fn spawn_ghoul(h: &mut RtsHarness, pos: [f32; 2]) -> EntityId {
    h.world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Ghoul), OWNER_ENEMY, pos)
        .expect("spawn ghoul")
}

fn select(h: &mut RtsHarness, ids: &[EntityId]) {
    h.world_mut().selection_mut().clear();
    for &id in ids {
        h.world_mut().selection_mut().insert(id);
    }
}

fn order_tag(h: &RtsHarness, id: EntityId) -> Option<u8> {
    h.world().order_of(id).map(|o| o.tag())
}

#[test]
fn attack_target_orders_armed_selection() {
    let mut h = scene();
    let s1 = spawn_soldier(&mut h, [30.5, 30.5]);
    let s2 = spawn_soldier(&mut h, [36.5, 30.5]);
    let ghoul = spawn_ghoul(&mut h, [60.5, 60.5]);
    select(&mut h, &[s1, s2]);
    let mut r = OrderReceiptBuffer::new();
    let receipt = h.world_mut().cmd_attack_target(ghoul, &mut r);
    assert_eq!(receipt.accepted, 2);
    assert_eq!(receipt.rejected, 0);
    assert!(receipt.reason.is_none());
    assert_eq!(r.as_slice().len(), 2);
    for rec in r.as_slice() {
        assert_eq!(rec.order, IssuedOrder::Attack);
    }
    // Both get tag 4 (Order::Attack)
    assert_eq!(order_tag(&h, s1), Some(4));
    assert_eq!(order_tag(&h, s2), Some(4));
}

#[test]
fn attack_target_walks_then_kills() {
    let mut h = scene();
    let soldier = spawn_soldier(&mut h, [30.5, 30.5]);
    let ghoul = spawn_ghoul(&mut h, [60.5, 60.5]);
    select(&mut h, &[soldier]);
    let mut r = OrderReceiptBuffer::new();
    let receipt = h.world_mut().cmd_attack_target(ghoul, &mut r);
    assert_eq!(receipt.accepted, 1);
    // Step until the ghoul is dead or bound expires
    for _ in 0..3600 {
        if h.world().entities().slot(ghoul).is_none() {
            break;
        }
        h.step_exact(1);
    }
    assert!(
        h.world().entities().slot(ghoul).is_none(),
        "ghoul must die within 3600 ticks"
    );
    assert_eq!(h.world().kills(), 1);
    assert_eq!(order_tag(&h, soldier), Some(Order::Idle.tag()));
}

#[test]
fn attack_move_formation_semantics() {
    let mut h = scene();
    let ids: Vec<EntityId> = (0..4)
        .map(|i| spawn_soldier(&mut h, [30.5 + i as f32 * 7.0, 30.5]))
        .collect();
    select(&mut h, &ids);
    let mut r = OrderReceiptBuffer::new();
    use mmd_engine::scenario::Cell;
    let receipt = h.world_mut().cmd_attack_move(Cell { x: 60, y: 60 }, &mut r);
    assert_eq!(receipt.accepted, 4);
    assert!(receipt.reason.is_none());
    assert_eq!(r.as_slice().len(), 4);
    for rec in r.as_slice() {
        assert_eq!(rec.order, IssuedOrder::AttackMove);
    }
    // All get tag 5 (Order::AttackMove)
    for &id in &ids {
        assert_eq!(order_tag(&h, id), Some(5));
    }
    // Four distinct formation goal slots
    let cells: Vec<_> = ids
        .iter()
        .map(|&id| {
            if let Some(Order::AttackMove { goal, .. }) = h.world().order_of(id) {
                goal.slot
            } else {
                panic!("expected AttackMove");
            }
        })
        .collect();
    let total = cells.len();
    let distinct = {
        let mut seen = std::collections::HashSet::new();
        cells.iter().filter(|c| seen.insert((c.x, c.y))).count()
    };
    assert_eq!(distinct, total, "formation slots must all be distinct");
}

#[test]
fn workers_in_selection_move_dont_fight() {
    let mut h = scene();
    let worker = spawn_worker(&mut h, [30.5, 30.5]);
    let soldier = spawn_soldier(&mut h, [36.5, 30.5]);
    select(&mut h, &[worker, soldier]);
    let mut r = OrderReceiptBuffer::new();
    use mmd_engine::scenario::Cell;
    let receipt = h.world_mut().cmd_attack_move(Cell { x: 60, y: 60 }, &mut r);
    assert_eq!(receipt.accepted, 2);
    assert!(receipt.reason.is_none());
    // worker gets Move (tag 1), soldier gets AttackMove (tag 5)
    assert_eq!(order_tag(&h, worker), Some(1));
    assert_eq!(order_tag(&h, soldier), Some(5));
    let worker_receipt = r.as_slice().iter().find(|rec| rec.id == worker).unwrap();
    let soldier_receipt = r.as_slice().iter().find(|rec| rec.id == soldier).unwrap();
    assert_eq!(worker_receipt.order, IssuedOrder::Move);
    assert_eq!(soldier_receipt.order, IssuedOrder::AttackMove);
}

#[test]
fn attack_target_walks_workers_instead() {
    let mut h = scene();
    let worker = spawn_worker(&mut h, [30.5, 30.5]);
    let soldier = spawn_soldier(&mut h, [36.5, 30.5]);
    let ghoul = spawn_ghoul(&mut h, [60.5, 60.5]);
    select(&mut h, &[worker, soldier]);
    let mut r = OrderReceiptBuffer::new();
    let receipt = h.world_mut().cmd_attack_target(ghoul, &mut r);
    assert_eq!(receipt.accepted, 2);
    assert!(receipt.reason.is_none());
    // soldier attacks, worker moves
    assert_eq!(order_tag(&h, soldier), Some(4)); // Attack
    assert_eq!(order_tag(&h, worker), Some(1)); // Move
    let worker_receipt = r.as_slice().iter().find(|rec| rec.id == worker).unwrap();
    let soldier_receipt = r.as_slice().iter().find(|rec| rec.id == soldier).unwrap();
    assert_eq!(worker_receipt.order, IssuedOrder::Move);
    assert_eq!(soldier_receipt.order, IssuedOrder::Attack);
}

#[test]
fn attack_target_rejects_unarmed_dead_and_missing() {
    // (a) workers-only selection + live Ghoul → NoArmedUnits
    {
        let mut h = scene();
        let worker = spawn_worker(&mut h, [30.5, 30.5]);
        let ghoul = spawn_ghoul(&mut h, [60.5, 60.5]);
        select(&mut h, &[worker]);
        let mut r = OrderReceiptBuffer::new();
        let receipt = h.world_mut().cmd_attack_target(ghoul, &mut r);
        assert_eq!(receipt.accepted, 0);
        assert_eq!(receipt.reason, Some(CommandRejectReason::NoArmedUnits));
        assert!(r.as_slice().is_empty());
    }
    // (b) soldiers + despawned Ghoul id → NoTarget
    {
        let mut h = scene();
        let soldier = spawn_soldier(&mut h, [30.5, 30.5]);
        let ghoul = spawn_ghoul(&mut h, [60.5, 60.5]);
        select(&mut h, &[soldier]);
        // Kill ghoul
        h.world_mut().apply_damage(ghoul, 100_000);
        h.step_exact(1);
        let mut r = OrderReceiptBuffer::new();
        let receipt = h.world_mut().cmd_attack_target(ghoul, &mut r);
        assert_eq!(receipt.accepted, 0);
        assert_eq!(receipt.reason, Some(CommandRejectReason::NoTarget));
        assert!(r.as_slice().is_empty());
    }
    // (c) soldiers + player-owned target (soldier_id) → NoTarget
    {
        let mut h = scene();
        let s1 = spawn_soldier(&mut h, [30.5, 30.5]);
        let s2 = spawn_soldier(&mut h, [36.5, 30.5]);
        select(&mut h, &[s1]);
        let mut r = OrderReceiptBuffer::new();
        let receipt = h.world_mut().cmd_attack_target(s2, &mut r);
        assert_eq!(receipt.accepted, 0);
        assert_eq!(receipt.reason, Some(CommandRejectReason::NoTarget));
        assert!(r.as_slice().is_empty());
    }
}

#[test]
fn stop_idles_and_cancels() {
    let mut h = scene();
    // Find a pre-existing worker in the scene
    let workers_ids = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    let worker = workers_ids[0];
    let soldier = spawn_soldier(&mut h, [36.5, 30.5]);
    let ghoul = spawn_ghoul(&mut h, [60.5, 60.5]);
    // Put soldier on an attack order
    select(&mut h, &[soldier]);
    let mut r = OrderReceiptBuffer::new();
    h.world_mut().cmd_attack_target(ghoul, &mut r);
    assert_eq!(order_tag(&h, soldier), Some(4));
    // Now stop both
    select(&mut h, &[worker, soldier]);
    let receipt = h.world_mut().cmd_stop(&mut r);
    assert_eq!(receipt.accepted, 2);
    assert_eq!(receipt.rejected, 0);
    assert!(receipt.reason.is_none());
    assert_eq!(order_tag(&h, worker), Some(Order::Idle.tag()));
    assert_eq!(order_tag(&h, soldier), Some(Order::Idle.tag()));
    assert_eq!(r.as_slice().len(), 2);
    for rec in r.as_slice() {
        assert_eq!(rec.order, IssuedOrder::Stop);
    }
}

#[test]
fn enemy_selection_rejects_commands() {
    let mut h = scene();
    let ghoul = spawn_ghoul(&mut h, [60.5, 60.5]);
    h.world_mut().selection_mut().clear();
    h.world_mut().selection_mut().insert(ghoul);
    let mut r = OrderReceiptBuffer::new();

    let receipt = h.world_mut().cmd_attack_target(ghoul, &mut r);
    assert_eq!(receipt.accepted, 0);
    assert_eq!(receipt.reason, Some(CommandRejectReason::EmptySelection));
    assert!(r.as_slice().is_empty());

    use mmd_engine::scenario::Cell;
    let receipt2 = h.world_mut().cmd_attack_move(Cell { x: 40, y: 40 }, &mut r);
    assert_eq!(receipt2.accepted, 0);
    assert_eq!(receipt2.reason, Some(CommandRejectReason::EmptySelection));

    let receipt3 = h.world_mut().cmd_stop(&mut r);
    assert_eq!(receipt3.accepted, 0);
    assert_eq!(receipt3.reason, Some(CommandRejectReason::EmptySelection));

    // Ghoul order is untouched
    assert_eq!(h.world().order_of(ghoul), Some(Order::Idle));
}

#[test]
fn command_receipts_do_not_grow_the_buffer() {
    let mut h = scene();
    let soldier = spawn_soldier(&mut h, [30.5, 30.5]);
    let ghoul = spawn_ghoul(&mut h, [60.5, 60.5]);
    select(&mut h, &[soldier]);
    let mut r = OrderReceiptBuffer::new();
    let cap_before = r.capacity();

    h.world_mut().cmd_attack_target(ghoul, &mut r);
    assert_eq!(r.capacity(), cap_before);

    use mmd_engine::scenario::Cell;
    h.world_mut().cmd_attack_move(Cell { x: 40, y: 40 }, &mut r);
    assert_eq!(r.capacity(), cap_before);

    h.world_mut().cmd_stop(&mut r);
    assert_eq!(r.capacity(), cap_before);
}
