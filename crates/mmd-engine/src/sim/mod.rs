//! Fixed-tick SoA agent simulation.

mod agents;
mod spatial;
mod tick;

pub use agents::{AgentsView, Simulation, quantize_cell};
pub use spatial::SpatialGrid;
pub use tick::{ARRIVAL_RADIUS, SPEED_CELLS_PER_SEC, TICK_DT};
