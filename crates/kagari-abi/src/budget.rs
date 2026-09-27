use serde::{Deserialize, Serialize};

/// One observable execution charge, checked before its logical operation.
///
/// The initial contract deliberately has no batched or zero-cost variant. A
/// backend may emit any number of machine operations for a MIR point, but must
/// charge this step once, at the original failure point, including when the
/// operation itself is optimized away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogicalBudgetCharge {
    Step,
}

impl LogicalBudgetCharge {
    pub const fn instruction_steps(self) -> u64 {
        match self {
            Self::Step => 1,
        }
    }
}
