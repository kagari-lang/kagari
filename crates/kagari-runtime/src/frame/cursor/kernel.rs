//! Closed scalar operations reuse authority without admitting callbacks or values.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::{cursor::ExecutionCursor, transfer::ReturnValue},
    module::execution::{ExecutionInstruction, ScalarSlot},
};

pub enum RegionExit {
    Slice,
    Safepoint,
    Boundary,
    Return(ReturnValue),
}

enum CursorProgress {
    Continue,
    Boundary,
    Return(ReturnValue),
}

impl ExecutionCursor<'_> {
    /// Execute sealed scalar operations under one authority check. The driver
    /// has already polled and observed the first PC; subsequent logical PCs keep
    /// the same cancellation, collection and observation boundaries. No caller
    /// supplies values or callbacks while authority is reused.
    pub fn execute_region(
        &mut self,
        remaining: &mut Option<usize>,
    ) -> Result<RegionExit, RuntimeError> {
        self.runtime
            .resources()
            .ensure_cursor_allowed(&self.session)?;
        self.runtime.gc().ensure_no_native_borrow()?;
        // A closed region cannot allocate/drop managed values, mutate executable
        // metadata or change collector policy. Recompute after every boundary.
        // Abandoned program leases can expire on another thread and remain
        // checked at each logical PC, as do cancellation and observer requests.
        let collection_due = self.runtime.gc().collection_due();
        let mut first = true;
        loop {
            if !first && *remaining == Some(0) {
                return Ok(RegionExit::Slice);
            }
            if !first && self.prepare_instruction(collection_due)? {
                return Ok(RegionExit::Safepoint);
            }
            first = false;
            if let Some(remaining) = remaining {
                *remaining = remaining.saturating_sub(1);
            }
            match self.execute_next()? {
                CursorProgress::Continue => {}
                CursorProgress::Boundary => return Ok(RegionExit::Boundary),
                CursorProgress::Return(value) => return Ok(RegionExit::Return(value)),
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
                    Some(slot) if slot.managed() => return Ok(CursorProgress::Boundary),
                    Some(slot) => ReturnValue::scalar(
                        representation,
                        self.payload(slot.scalar().expect("scalar return"))?,
                    ),
                    None => ReturnValue::scalar(representation, 0),
                };
                return Ok(CursorProgress::Return(value));
            }
            ExecutionInstruction::Boundary => return Ok(CursorProgress::Boundary),
        };
        self.values
            .write_payload(&self.ranges, dst, value)
            .ok_or_else(|| self.invalid())?;
        Ok(CursorProgress::Continue)
    }

    #[inline]
    fn payload(&self, slot: ScalarSlot) -> Result<u64, RuntimeError> {
        self.values
            .payload(&self.ranges, slot)
            .ok_or_else(|| self.invalid())
    }
}
