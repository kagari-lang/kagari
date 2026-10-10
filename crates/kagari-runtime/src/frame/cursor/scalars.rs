//! Borrow code and scalar banks once while preserving each logical boundary.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::{cursor::ExecutionCursor, transfer::ReturnValue},
    module::execution::{ExecutionInstruction, ScalarSlot, managed::PreparedManagedOperation},
};

pub(super) enum ScalarExit {
    Slice,
    Safepoint,
    Boundary,
    Return(ReturnValue),
    Managed(PreparedManagedOperation),
}

/// The scalar segment selects its mode once at entry. Only bounded
/// execution carries writable step state; both modes use the same instruction loop.
trait InstructionSlice {
    fn exhausted(&self) -> bool;

    fn advance(&mut self);
}

struct Unbounded;

impl InstructionSlice for Unbounded {
    fn exhausted(&self) -> bool {
        false
    }

    fn advance(&mut self) {}
}

struct Bounded<'a>(&'a mut usize);

impl InstructionSlice for Bounded<'_> {
    fn exhausted(&self) -> bool {
        *self.0 == 0
    }

    fn advance(&mut self) {
        *self.0 = self.0.saturating_sub(1);
    }
}

impl<'code> ExecutionCursor<'code> {
    pub(super) fn execute_scalars(
        &mut self,
        remaining: &mut Option<usize>,
        collection_due: bool,
        first: bool,
    ) -> Result<ScalarExit, RuntimeError> {
        match remaining {
            Some(remaining) => self.execute_slice(&mut Bounded(remaining), collection_due, first),
            None => self.execute_slice(&mut Unbounded, collection_due, first),
        }
    }

    // Keep object handlers out of this loop's register allocation and inlining
    // budget. An unbounded segment has no optional countdown to load or spill.
    #[inline(never)]
    fn execute_slice<S: InstructionSlice>(
        &mut self,
        slice: &mut S,
        collection_due: bool,
        mut first: bool,
    ) -> Result<ScalarExit, RuntimeError> {
        loop {
            if !first && slice.exhausted() {
                return Ok(ScalarExit::Slice);
            }
            if !first && self.prepare_instruction(collection_due) {
                return Ok(ScalarExit::Safepoint);
            }
            first = false;
            slice.advance();
            let instruction = self.next_instruction().ok_or_else(|| {
                self.runtime
                    .resources()
                    .quarantine("verified function fell through")
            })?;
            let (dst, value) = match *instruction {
                ExecutionInstruction::Constant { dst, value } => (dst, value),
                ExecutionInstruction::Move { dst, src } => (dst, self.payload(src)?),
                ExecutionInstruction::Scalar {
                    dst,
                    lhs,
                    rhs,
                    kernel,
                } => {
                    let lhs = self.payload(lhs)?;
                    let rhs = self.payload(rhs)?;
                    let value = kernel(lhs, rhs).map_err(|reason| {
                        RuntimeError::new(RuntimeErrorKind::ScriptTrap, reason)
                    })?;
                    (dst, value)
                }
                ExecutionInstruction::Jump(target) => {
                    self.jump(target.index())?;
                    continue;
                }
                ExecutionInstruction::Branch {
                    cond,
                    then_target,
                    else_target,
                } => {
                    let target = match self.payload(cond)? {
                        1 => then_target,
                        0 => else_target,
                        _ => {
                            return Err(self
                                .runtime
                                .resources()
                                .quarantine("verified branch condition is not bool"));
                        }
                    };
                    self.jump(target.index())?;
                    continue;
                }
                ExecutionInstruction::Return {
                    value,
                    representation,
                } => {
                    let value = match value {
                        Some(slot) => ReturnValue::scalar(representation, self.payload(slot)?),
                        None => ReturnValue::scalar(representation, 0),
                    };
                    return Ok(ScalarExit::Return(value));
                }
                ExecutionInstruction::Managed(operation) => {
                    return Ok(ScalarExit::Managed(operation));
                }
                ExecutionInstruction::Boundary => return Ok(ScalarExit::Boundary),
            };
            self.write_payload(dst, value)
                .ok_or_else(|| self.invalid())?;
        }
    }

    #[inline(always)]
    fn payload(&self, slot: ScalarSlot) -> Result<u64, RuntimeError> {
        if !self
            .values
            .initialized
            .get(slot.index())
            .copied()
            .unwrap_or(false)
        {
            return Err(self.invalid());
        }
        self.values
            .payloads
            .get(slot.index())
            .copied()
            .ok_or_else(|| self.invalid())
    }

    fn write_payload(&mut self, slot: ScalarSlot, value: u64) -> Option<()> {
        *self.values.payloads.get_mut(slot.index())? = value;
        *self.values.initialized.get_mut(slot.index())? = true;
        Some(())
    }

    fn prepare_instruction(&mut self, collection_due: bool) -> bool {
        *self.executing = None;
        self.session.options.cancellation.check().is_err()
            || self.session.observer_attached.get()
            || collection_due
            || (self.runtime.gc().automatic_collection_enabled()
                && self.runtime.modules.abandonment_pending())
    }

    fn next_instruction(&mut self) -> Option<&'code ExecutionInstruction> {
        // The admitted code slice is immutable for this region. Borrow the
        // instruction so only the selected handler reads its payload; copying
        // the whole enum before dispatch also decodes unrelated managed fields.
        let instruction = self.function.instructions.get(*self.ip)?;
        *self.executing = Some(*self.ip);
        *self.ip += 1;
        Some(instruction)
    }

    fn jump(&mut self, target: usize) -> Result<(), RuntimeError> {
        if target >= self.function.instructions.len() {
            return Err(self
                .runtime
                .resources()
                .quarantine("invalid frame jump target"));
        }
        *self.ip = target;
        Ok(())
    }
}
