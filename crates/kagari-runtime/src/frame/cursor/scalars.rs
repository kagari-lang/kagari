//! Borrow code and scalar banks once while preserving each logical boundary.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::{cursor::ExecutionCursor, transfer::ReturnValue},
    module::execution::{ExecutionInstruction, ScalarSlot, managed::PreparedManagedOperation},
    session::SessionState,
};
use kagari_bytecode::module::CallableTarget;

pub(super) struct ScalarCursor<'a> {
    instructions: &'a [ExecutionInstruction],
    ip: &'a mut usize,
    executing: &'a mut Option<usize>,
    payloads: &'a mut [u64],
    initialized: &'a mut [bool],
    runtime: &'a Runtime,
    session: &'a SessionState,
}

pub(super) enum ScalarExit {
    Slice,
    Safepoint,
    Boundary,
    Return(ReturnValue),
    Managed(PreparedManagedOperation),
}

enum CursorProgress {
    Continue,
    Boundary,
    Return(ReturnValue),
    Managed(PreparedManagedOperation),
}

impl ExecutionCursor<'_> {
    pub(super) fn scalars(&mut self) -> Result<ScalarCursor<'_>, RuntimeError> {
        let frame = &mut *self.frame;
        let CallableTarget::Script(function) = frame.target else {
            return Err(self
                .runtime
                .resources()
                .quarantine("scalar region requires script frame"));
        };
        let instructions = &frame
            .loaded
            .execution()
            .functions
            .get(function.index())
            .ok_or_else(|| {
                self.runtime
                    .resources()
                    .quarantine("invalid execution function")
            })?
            .instructions;
        Ok(ScalarCursor {
            instructions,
            ip: &mut frame.ip,
            executing: &mut frame.executing,
            payloads: self.values.payloads,
            initialized: self.values.initialized,
            runtime: self.runtime,
            session: self.session,
        })
    }
}

impl ScalarCursor<'_> {
    // Keep object handlers out of this loop's register allocation and inlining
    // budget while reusing the same admitted cursor across both operation kinds.
    #[inline(never)]
    pub(super) fn execute(
        &mut self,
        remaining: &mut Option<usize>,
        collection_due: bool,
        mut first: bool,
    ) -> Result<ScalarExit, RuntimeError> {
        loop {
            if !first && *remaining == Some(0) {
                return Ok(ScalarExit::Slice);
            }
            if !first && self.prepare_instruction(collection_due) {
                return Ok(ScalarExit::Safepoint);
            }
            first = false;
            if let Some(remaining) = remaining {
                *remaining = remaining.saturating_sub(1);
            }
            match self.execute_next()? {
                CursorProgress::Continue => {}
                CursorProgress::Boundary => return Ok(ScalarExit::Boundary),
                CursorProgress::Return(value) => {
                    return Ok(ScalarExit::Return(value));
                }
                CursorProgress::Managed(instruction) => {
                    return Ok(ScalarExit::Managed(instruction));
                }
            }
        }
    }

    fn execute_next(&mut self) -> Result<CursorProgress, RuntimeError> {
        let instruction = self.next_instruction().ok_or_else(|| {
            self.runtime
                .resources()
                .quarantine("verified function fell through")
        })?;
        let (dst, value) = match instruction {
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
                let value = kernel(lhs, rhs)
                    .map_err(|reason| RuntimeError::new(RuntimeErrorKind::ScriptTrap, reason))?;
                (dst, value)
            }
            ExecutionInstruction::Jump(target) => {
                self.jump(target.index())?;
                return Ok(CursorProgress::Continue);
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
                return Ok(CursorProgress::Continue);
            }
            ExecutionInstruction::Return {
                value,
                representation,
            } => {
                let value = match value {
                    Some(slot) => ReturnValue::scalar(representation, self.payload(slot)?),
                    None => ReturnValue::scalar(representation, 0),
                };
                return Ok(CursorProgress::Return(value));
            }
            ExecutionInstruction::Managed(operation) => {
                return Ok(CursorProgress::Managed(operation));
            }
            ExecutionInstruction::Boundary => return Ok(CursorProgress::Boundary),
        };
        self.write_payload(dst, value)
            .ok_or_else(|| self.invalid())?;
        Ok(CursorProgress::Continue)
    }

    #[inline(always)]
    fn payload(&self, slot: ScalarSlot) -> Result<u64, RuntimeError> {
        if !self.initialized.get(slot.index()).copied().unwrap_or(false) {
            return Err(self.invalid());
        }
        self.payloads
            .get(slot.index())
            .copied()
            .ok_or_else(|| self.invalid())
    }

    fn write_payload(&mut self, slot: ScalarSlot, value: u64) -> Option<()> {
        *self.payloads.get_mut(slot.index())? = value;
        *self.initialized.get_mut(slot.index())? = true;
        Some(())
    }

    fn invalid(&self) -> RuntimeError {
        self.runtime
            .resources()
            .quarantine("invalid execution operand slot")
    }

    fn prepare_instruction(&mut self, collection_due: bool) -> bool {
        *self.executing = None;
        self.session.options.cancellation.check().is_err()
            || self.session.observer_attached.get()
            || collection_due
            || (self.runtime.gc().automatic_collection_enabled()
                && self.runtime.modules.abandonment_pending())
    }

    fn next_instruction(&mut self) -> Option<ExecutionInstruction> {
        let instruction = self.instructions.get(*self.ip).copied()?;
        *self.executing = Some(*self.ip);
        *self.ip += 1;
        Some(instruction)
    }

    fn jump(&mut self, target: usize) -> Result<(), RuntimeError> {
        if target >= self.instructions.len() {
            return Err(self
                .runtime
                .resources()
                .quarantine("invalid frame jump target"));
        }
        *self.ip = target;
        Ok(())
    }
}
