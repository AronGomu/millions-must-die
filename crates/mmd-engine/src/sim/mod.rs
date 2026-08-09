//! Fixed-tick SoA agent simulation.

mod agents;
mod collision;
mod pool;
mod spatial;
mod tick;

pub use agents::{AgentsView, Simulation, quantize_cell};
pub use collision::{
    COINCIDENT_EPS2, CollisionParams, MAX_SEPARATION_NEIGHBORS, SEPARATION_DIR16,
    accumulate_separation, accumulate_separation_phase, accumulate_separation_range,
};
pub use spatial::SpatialGrid;
pub use tick::{ARRIVAL_RADIUS, SPEED_CELLS_PER_SEC, TICK_DT};
