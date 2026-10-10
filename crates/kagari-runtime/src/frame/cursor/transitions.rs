//! Allocating preparation runs only after closed-region borrows have ended.
use crate::{
    Runtime,
    frame::{
        ExecutionStack,
        cursor::kernel::{PreparedTransition, RegionError, RegionExit},
    },
};

impl ExecutionStack<'_> {
    // Preserve the semantic boundary in generated code: ordinary region admission
    // must not inherit the stack and code footprint of layout/allocation handling.
    #[inline(never)]
    pub(super) fn complete_transition(
        &self,
        runtime: &Runtime,
        transition: PreparedTransition,
    ) -> Result<RegionExit, RegionError> {
        match transition {
            PreparedTransition::Field { index } => self.execute_scoped_field(runtime, index),
            PreparedTransition::Constant { dst, constant } => {
                // The cursor and all frame/bank/session borrows have ended. The
                // runtime-owned frame still roots the supplying program. This is
                // the cold allocation transition, not a second logical instruction.
                let (owner, pool) = {
                    let frame = self.current()?;
                    let links = frame.links.as_ref().ok_or_else(|| {
                        runtime
                            .resources()
                            .quarantine("missing constant execution link")
                    })?;
                    (frame.loaded.clone(), links.constants.clone())
                };
                let value = pool.materialize(runtime, &owner, constant)?;
                let frame = self.current()?;
                if !runtime.gc().validate_value(&value) {
                    return Err(runtime
                        .resources()
                        .quarantine("invalid constant value")
                        .into());
                }
                runtime
                    .resources()
                    .frame_values
                    .try_borrow_mut()
                    .map_err(|_| {
                        runtime
                            .resources()
                            .quarantine("execution slots borrowed during constant publication")
                    })?
                    .set_location(frame.slots, dst, value)
                    .ok_or_else(|| {
                        runtime
                            .resources()
                            .quarantine("invalid constant destination")
                    })?;
                // Recompute allocation/collection state at the successor PC.
                Ok(RegionExit::Safepoint)
            }
        }
    }
}
