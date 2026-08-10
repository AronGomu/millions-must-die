//! Fixed 60 Hz movement + recycle step.

use super::agents::Simulation;

/// Sim speed: 8 cells per second.
pub const SPEED_CELLS_PER_SEC: f32 = 8.0;
/// Fixed tick duration (seconds).
pub const TICK_DT: f32 = 1.0 / 60.0;
/// Arrival radius from destination center (cells).
pub const ARRIVAL_RADIUS: f32 = 0.5;

/// Squared length below which a blended steering vector is treated as
/// cancelled, and the pure descent direction is kept instead.
const BLEND_EPS2: f32 = 1e-12;

/// Advance all agents one tick.
///
/// One move per agent, always. When the scenario declares a body the descent
/// vector is first bent by the neighbours pressing on the agent (see
/// [`super::collision`]); when it does not, no neighbour is ever queried and
/// the walk is pure flow-field descent. When the scenario spreads the pass
/// over several ticks, an agent outside this tick's phase keeps the
/// repulsion it was last given, and the neighbour scan itself always reads
/// live positions — but through a spatial grid that is only rebuilt once per
/// cycle. Bin *membership* can therefore be up to `separation_phases - 1`
/// ticks stale: a neighbour that has since crossed into or out of an
/// agent's 3x3 scan window is missed or wrongly included until the next
/// rebuild. An agent recycled to a spawn cell mid-cycle keeps steering by a
/// repulsion measured at its old position until its phase comes back around.
pub fn step(sim: &mut Simulation) {
    let width = sim.width;
    let height = sim.height;
    let dest_cx = sim.dest_cx;
    let dest_cy = sim.dest_cy;
    let dest_x = sim.dest_x;
    let dest_y = sim.dest_y;
    let step_len = SPEED_CELLS_PER_SEC * TICK_DT;
    let arrival_r2 = ARRIVAL_RADIUS * ARRIVAL_RADIUS;
    let frame_count = sim.frame_count;
    let n_spawn = sim.spawn_x.len();
    let n = sim.x.len();

    let collision = sim.collision;
    let collision_on = collision.enabled();
    if collision_on {
        // One cadence for both halves of the pass. Reynolds' `skipThink`
        // recomputes less, it does not spread the same work thinner — the
        // scan that is skipped is never run.
        let phases = collision.phases.max(1) as u64;
        let phase = sim.tick_index % phases;
        if phase == 0 {
            sim.grid.rebuild(&sim.x, &sim.y);
            sim.grid_rebuilds += 1;
        }
        // Cloning the `Arc` releases the borrow on `sim` before the job's
        // pointers are taken; it is a refcount bump, not an allocation. It is
        // also load-bearing for soundness, not just for borrowck: holding an
        // owned `Arc` across the whole of `run` is what makes it impossible for
        // the pool to be dropped — and its workers joined — while a job that
        // points into this simulation is still live.
        //
        // Both arms compute the same thing. The pool exists to spread the pass,
        // not to change it.
        match sim.pool.clone() {
            Some(pool) => pool.run(super::pool::job_for(sim, phases as u32, phase as u32)),
            None => super::collision::accumulate_separation_phase(
                &sim.x,
                &sim.y,
                &sim.grid,
                collision.radius_cells,
                &sim.mass,
                &sim.inv_mass,
                phases as u32,
                phase as u32,
                &mut sim.sep_x,
                &mut sim.sep_y,
            ),
        }
    }

    for i in 0..n {
        let px = sim.x[i];
        let py = sim.y[i];

        // Arrival vs destination center.
        let adx = px - dest_x;
        let ady = py - dest_y;
        if adx * adx + ady * ady <= arrival_r2 {
            recycle_one(sim, i, n_spawn);
            continue;
        }

        // Nearest-cell sample (cell centers → floor of continuous pos).
        let cx = nearest_cell(px);
        let cy = nearest_cell(py);
        if cx < 0 || cy < 0 || cx >= width as i32 || cy >= height as i32 {
            advance_frame(&mut sim.frame[i], frame_count);
            continue;
        }

        // Destination cell (zero vector) → recycle even if corner > radius.
        if cx as u32 == dest_cx && cy as u32 == dest_cy {
            recycle_one(sim, i, n_spawn);
            continue;
        }

        let cidx = (cx as u32 + cy as u32 * width) as usize;
        let vx = sim.field_vx[cidx];
        let vy = sim.field_vy[cidx];

        if vx == 0.0 && vy == 0.0 {
            advance_frame(&mut sim.frame[i], frame_count);
            continue;
        }

        // Steering blend: the descent direction bent by the neighbours pressing
        // on this agent, walked at the unchanged speed. Skipped entirely when
        // the scenario declares no body, so a bodyless run stays bit-identical
        // to a pure flow-field walk.
        let (mut mx, mut my) = (vx, vy);
        if collision_on {
            let bx = vx + collision.strength * sim.sep_x[i];
            let by = vy + collision.strength * sim.sep_y[i];
            let l2 = bx * bx + by * by;
            if l2 > BLEND_EPS2 {
                let inv = 1.0 / l2.sqrt();
                mx = bx * inv;
                my = by * inv;
            }
        }

        let mut nx = px + mx * step_len;
        let mut ny = py + my * step_len;

        // Separation must never wedge an agent the field alone could have
        // moved: fall back to the pure descent step before giving up. Without
        // this, a crowd could pin an agent against a wall forever.
        if !step_admissible(cx, cy, nx, ny, width, height, &sim.blocked) {
            mx = vx;
            my = vy;
            nx = px + mx * step_len;
            ny = py + my * step_len;
            if !step_admissible(cx, cy, nx, ny, width, height, &sim.blocked) {
                sim.dir[i] = dir_from_vector(mx, my);
                advance_frame(&mut sim.frame[i], frame_count);
                continue;
            }
        }

        sim.x[i] = nx;
        sim.y[i] = ny;
        sim.dir[i] = dir_from_vector(mx, my);
        advance_frame(&mut sim.frame[i], frame_count);
    }

    sim.tick_index = sim.tick_index.wrapping_add(1);
}

fn recycle_one(sim: &mut Simulation, i: usize, n_spawn: usize) {
    let si = (sim.recycle_cursor as usize) % n_spawn;
    sim.recycle_cursor = sim.recycle_cursor.wrapping_add(1);
    sim.x[i] = sim.spawn_x[si];
    sim.y[i] = sim.spawn_y[si];
}

#[inline]
fn nearest_cell(p: f32) -> i32 {
    // Nearest cell center at n+0.5; Voronoi of centers ≡ floor(p).
    p.floor() as i32
}

/// Is a step from cell `(cx, cy)` to the continuous position `(nx, ny)` one the
/// walk is allowed to take?
///
/// Two conditions, and the second is why this is not a plain walkability test.
/// The destination cell must be in bounds and free — and if the step carries the
/// centre across *both* cell boundaries at once, it is a diagonal move and must
/// satisfy the flow field's own no-corner-cut rule: both shared cardinal
/// neighbours clear. The pure descent step satisfies that by construction,
/// because [`crate::nav::flow_field`] only ever emits a diagonal vector through
/// the same check; a *blended* step is an arbitrary unit vector and does not.
///
/// Without the diagonal arm, separation can steer an agent across a blocked
/// corner into a cell that is walkable but unreachable. Such a cell has a zero
/// descent vector forever, so the agent parked there never moves, never arrives
/// and never recycles — and the wall fallback above cannot rescue it, because
/// the blended step *was* walkable.
#[inline]
pub(crate) fn step_admissible(
    cx: i32,
    cy: i32,
    nx: f32,
    ny: f32,
    width: u32,
    height: u32,
    blocked: &[bool],
) -> bool {
    let tx = nearest_cell(nx);
    let ty = nearest_cell(ny);
    if tx < 0 || ty < 0 || tx >= width as i32 || ty >= height as i32 {
        return false;
    }
    let idx = (tx as u32 + ty as u32 * width) as usize;
    if blocked[idx] {
        return false;
    }
    // `signum`, not the raw delta: a step is shorter than a cell so the delta is
    // already in -1..=1, but the rule is about the corner being crossed and must
    // not silently index a cell two away if that ever stops holding.
    let dx = (tx - cx).signum();
    let dy = (ty - cy).signum();
    if dx != 0 && dy != 0 {
        return crate::nav::flow_field::diagonal_clear(cx, cy, dx, dy, width, height, blocked);
    }
    true
}

#[inline]
fn advance_frame(frame: &mut u8, frame_count: u8) {
    *frame = (*frame + 1) % frame_count;
}

/// Map velocity to 8-way dir: 0=E,1=NE,2=N,3=NW,4=W,5=SW,6=S,7=SE.
#[inline]
pub fn dir_from_vector(vx: f32, vy: f32) -> u8 {
    // Invert y so grid-north (0,-1) maps to math +Y.
    let ang = (-vy).atan2(vx); // -pi..pi, 0 = east
    const TAU: f32 = 2.0 * std::f32::consts::PI;
    let mut t = ang / TAU; // -0.5..0.5
    if t < 0.0 {
        t += 1.0;
    }
    // t=0 east, increases toward north (CCW in inverted-y space).
    let sector = (t * 8.0 + 0.5).floor() as i32;
    (((sector % 8) + 8) % 8) as u8
}
