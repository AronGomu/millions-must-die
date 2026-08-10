//! Player resource stock and supply accounting.

use super::entity::UnitKind;
use crate::scenario::MAX_SUPPLY_CAP;

/// Player stock of both resources.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Resources {
    pub crystal: u32,
    pub gas: u32,
}

impl Resources {
    pub const ZERO: Self = Self { crystal: 0, gas: 0 };

    /// Whether this stock covers `cost`.
    pub fn covers(&self, cost: Resources) -> bool {
        self.crystal >= cost.crystal && self.gas >= cost.gas
    }

    /// Subtract `cost`, or leave the stock untouched and return `false`.
    pub fn try_debit(&mut self, cost: Resources) -> bool {
        if !self.covers(cost) {
            return false;
        }
        self.crystal -= cost.crystal;
        self.gas -= cost.gas;
        true
    }

    /// Add `amount`, saturating.
    pub fn credit(&mut self, amount: Resources) {
        self.crystal = self.crystal.saturating_add(amount.crystal);
        self.gas = self.gas.saturating_add(amount.gas);
    }
}

/// Supply usage and ceiling. `cap` is clamped to `scenario::MAX_SUPPLY_CAP`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Supply {
    used: u32,
    cap: u32,
}

impl Supply {
    /// Clamps `cap` to `MAX_SUPPLY_CAP`.
    pub fn new(cap: u32) -> Self {
        Self {
            used: 0,
            cap: cap.min(MAX_SUPPLY_CAP),
        }
    }

    pub fn used(&self) -> u32 {
        self.used
    }

    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// Free supply headroom, saturating at zero — a cap that *fell* below usage
    /// (a Depot destroyed later) must read as 0 free, not underflow.
    pub fn free(&self) -> u32 {
        self.cap.saturating_sub(self.used)
    }

    /// Whether `cost` fits under the cap right now.
    pub fn fits(&self, cost: u32) -> bool {
        cost <= self.free()
    }

    pub fn add_used(&mut self, cost: u32) {
        self.used = self.used.saturating_add(cost);
    }

    pub fn remove_used(&mut self, cost: u32) {
        self.used = self.used.saturating_sub(cost);
    }

    /// Clamps to `MAX_SUPPLY_CAP`.
    pub fn grant_cap(&mut self, amount: u32) {
        self.cap = self.cap.saturating_add(amount).min(MAX_SUPPLY_CAP);
    }

    pub fn revoke_cap(&mut self, amount: u32) {
        self.cap = self.cap.saturating_sub(amount);
    }
}

/// Supply a unit kind costs.
pub const WORKER_SUPPLY_COST: u32 = 1;
pub const SOLDIER_SUPPLY_COST: u32 = 2;

pub fn supply_cost(kind: UnitKind) -> u32 {
    match kind {
        UnitKind::Worker => WORKER_SUPPLY_COST,
        UnitKind::Soldier => SOLDIER_SUPPLY_COST,
    }
}
