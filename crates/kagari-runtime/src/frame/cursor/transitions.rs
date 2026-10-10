//! Allocating preparation runs only after closed-region borrows have ended.
use crate::{
    Runtime,
    frame::{
        ExecutionStack,
        cursor::kernel::{PreparedTransition, RegionError, RegionExit},
    },
    module::execution::indices::IndexAccess,
    value::Value,
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
            PreparedTransition::TupleWrite { operation, index } => {
                // Keep cold transition requests small: operands belong to the
                // rooted frame, not a second value packet passed through every
                // ordinary region exit. Nothing can mutate that frame between
                // cursor exit and this private transition.
                let (base, tuple, value) = {
                    let frame = self.current()?;
                    let invalid = || runtime.resources().quarantine("invalid tuple transition");
                    let operation = frame.prepared_index(operation).ok_or_else(invalid)?;
                    let IndexAccess::Write { value } = operation.access else {
                        return Err(invalid().into());
                    };
                    let values = runtime
                        .resources()
                        .frame_values
                        .try_borrow()
                        .map_err(|_| invalid())?;
                    let Value::Tuple(tuple) = values
                        .with_location(frame.slots, operation.base, |value| *value)
                        .ok_or_else(invalid)?
                    else {
                        return Err(invalid().into());
                    };
                    let value = values
                        .with_location(frame.slots, value, |value| *value)
                        .ok_or_else(invalid)?;
                    (operation.base, tuple, value)
                };
                // Both the old tuple and replacement value still reside in the
                // active frame window. Allocation cannot collect or call back;
                // install the replacement root before the successor safepoint.
                let mut elements = runtime
                    .gc()
                    .tuple(tuple)
                    .ok_or(RegionError::InvalidIndex(index))?
                    .to_vec();
                let slot = elements
                    .get_mut(index)
                    .ok_or(RegionError::InvalidIndex(index))?;
                *slot = value;
                let tuple = runtime.gc().alloc_tuple(elements)?;
                let frame = self.current()?;
                if !runtime.gc().validate_value(&tuple) {
                    return Err(runtime.resources().quarantine("invalid tuple value").into());
                }
                runtime
                    .resources()
                    .frame_values
                    .try_borrow_mut()
                    .map_err(|_| {
                        runtime
                            .resources()
                            .quarantine("tuple operands borrowed during publication")
                    })?
                    .set_location(frame.slots, base, tuple)
                    .ok_or_else(|| runtime.resources().quarantine("invalid tuple destination"))?;
                Ok(RegionExit::Safepoint)
            }
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
