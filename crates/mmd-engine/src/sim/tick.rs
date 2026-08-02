//! Fixed 60 Hz movement + recycle step.

use super::agents::Simulation;

/// Sim speed: 8 cells per second.
pub const SPEED_CELLS_PER_SEC: f32 = 8.0;
/// Fixed tick duration (seconds).
pub const TICK_DT: f32 = 1.0 / 60.0;
/// Arrival radius from destination center (cells).
pub const ARRIVAL_RADIUS: f32 = 0.5;

/// Advance all agents one tick. No agent-agent queries.
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

        let nx = px + vx * step_len;
        let ny = py + vy * step_len;

        // OOB / obstacle next position → retain prior.
        if !position_walkable(nx, ny, width, height, &sim.blocked) {
            sim.dir[i] = dir_from_vector(vx, vy);
            advance_frame(&mut sim.frame[i], frame_count);
            continue;
        }

        sim.x[i] = nx;
        sim.y[i] = ny;
        sim.dir[i] = dir_from_vector(vx, vy);
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

#[inline]
fn position_walkable(x: f32, y: f32, width: u32, height: u32, blocked: &[bool]) -> bool {
    let cx = nearest_cell(x);
    let cy = nearest_cell(y);
    if cx < 0 || cy < 0 || cx >= width as i32 || cy >= height as i32 {
        return false;
    }
    let idx = (cx as u32 + cy as u32 * width) as usize;
    !blocked[idx]
}

#[inline]
fn advance_frame(frame: &mut u8, frame_count: u8) {
    *frame = (*frame + 1) % frame_count;
}

/// Map velocity to 8-way dir: 0=E,1=NE,2=N,3=NW,4=W,5=SW,6=S,7=SE.
#[inline]
fn dir_from_vector(vx: f32, vy: f32) -> u8 {
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
