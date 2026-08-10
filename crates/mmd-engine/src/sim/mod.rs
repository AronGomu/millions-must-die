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
/// The horde's step-admissibility rule, for the RTS mover to be tested
/// against. Test-only and crate-internal: the two implementations must be
/// provably equal (`rts::orders`), and that proof needs a name to compare
/// with. Nothing in a shipping build reaches the horde's rule from outside
/// [`tick`].
#[cfg(test)]
pub(crate) use tick::step_admissible;
pub use tick::{ARRIVAL_RADIUS, SPEED_CELLS_PER_SEC, TICK_DT, dir_from_vector};
