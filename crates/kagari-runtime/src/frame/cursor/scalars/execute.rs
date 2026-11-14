//! Direct scalar dispatch; only the marked instruction arms are generated.
use super::{InstructionSlice, ScalarCursor, ScalarExit, ScalarFailure};
use crate::{frame::transfer::ReturnValue, module::execution::instruction::ExecutionInstruction};
use kagari_types::{
    payload::{self, comparison, shared},
    scalar::BuiltinType,
};

impl ScalarCursor<'_, '_> {
    // Keep object handlers out of this loop's register allocation and inlining
    // budget. An unbounded segment has no optional countdown to load or spill.
    #[inline(never)]
    pub(super) fn execute_slice<S: InstructionSlice>(
        &mut self,
        slice: &mut S,
        collection_due: bool,
        mut first: bool,
    ) -> Result<ScalarExit, ScalarFailure> {
        // No callback or external frame access can observe these locals.
        // The common exit publishes both fields on success and failure.
        let mut next = *self.ip;
        let mut executing = *self.executing;
        let automatic_collection = self.automatic_collection;
        // Window admission takes both banks from the same range. Make
        // their shared bound visible across this non-inlined boundary.
        // All indexed accesses and initialization checks remain checked.
        if self.payloads.len() != self.initialized.len() {
            return Err(ScalarFailure::InconsistentBanks);
        }
        let result = 'segment: loop {
            macro_rules! attempt {
                ($result:expr) => {
                    match $result {
                        Ok(value) => value,
                        Err(failure) => break 'segment Err(failure),
                    }
                };
            }

            if !first && slice.exhausted() {
                break 'segment Ok(ScalarExit::Slice);
            }
            if !first {
                executing = None;
                if self.poll_requested(collection_due, automatic_collection) {
                    break 'segment Ok(ScalarExit::Safepoint);
                }
            }
            first = false;
            slice.advance();
            let instruction = attempt!(
                self.function
                    .instructions
                    .get(next)
                    .ok_or(ScalarFailure::UnexpectedFunctionEnd)
            );
            executing = Some(next);
            next += 1;
            let (dst, value) = match *instruction {
                ExecutionInstruction::Unit => continue,
                ExecutionInstruction::Constant { dst, value } => (dst, value),
                ExecutionInstruction::Move { dst, src } => (
                    dst,
                    attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot)),
                ),
                // BEGIN GENERATED scalar-execution
                // Source: codegen/scalar-instructions.toml. Regenerate with:
                // uv run python scripts/generate_scalar_execution.py
                ExecutionInstruction::I8AddRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8SubRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8MulRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8DivRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8RemRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8BitAndRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8BitOrRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8BitXorRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16AddRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16SubRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16MulRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16DivRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16RemRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16BitAndRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16BitOrRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16BitXorRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32AddRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32SubRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32MulRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32DivRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32RemRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32BitAndRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32BitOrRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32BitXorRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64AddRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64SubRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64MulRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64DivRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64RemRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64BitAndRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64BitOrRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64BitXorRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8AddRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8SubRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8MulRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8DivRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8RemRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8BitAndRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8BitOrRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8BitXorRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16AddRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16SubRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16MulRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16DivRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16RemRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16BitAndRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16BitOrRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16BitXorRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32AddRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32SubRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32MulRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32DivRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32RemRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32BitAndRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32BitOrRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32BitXorRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64AddRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64SubRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64MulRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64DivRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64RemRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64BitAndRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64BitOrRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64BitXorRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F32AddRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<0, true>(lhs, rhs).map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F32SubRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<1, true>(lhs, rhs).map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F32MulRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<2, true>(lhs, rhs).map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F32DivRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<3, true>(lhs, rhs).map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F32RemRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<4, true>(lhs, rhs).map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F64AddRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<0, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F64SubRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<1, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F64MulRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<2, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F64DivRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<3, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F64RemRR { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<4, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8AddRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8SubRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8MulRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8DivRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8RemRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8BitAndRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8BitOrRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8BitXorRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 8, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16AddRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16SubRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16MulRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16DivRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16RemRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16BitAndRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16BitOrRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16BitXorRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 16, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32AddRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32SubRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32MulRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32DivRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32RemRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32BitAndRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32BitOrRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32BitXorRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 32, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64AddRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64SubRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64MulRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64DivRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64RemRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64BitAndRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64BitOrRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64BitXorRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 64, true>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8AddRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8SubRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8MulRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8DivRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8RemRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8BitAndRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8BitOrRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8BitXorRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 8, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16AddRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16SubRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16MulRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16DivRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16RemRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16BitAndRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16BitOrRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16BitXorRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 16, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32AddRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32SubRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32MulRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32DivRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32RemRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32BitAndRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32BitOrRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32BitXorRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 32, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64AddRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::integer_value::<0, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64SubRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::integer_value::<1, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64MulRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::integer_value::<2, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64DivRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::integer_value::<3, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64RemRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::integer_value::<4, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64BitAndRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::integer_value::<5, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64BitOrRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::integer_value::<6, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64BitXorRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::integer_value::<7, 64, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F32AddRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<0, true>(lhs, rhs).map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F32SubRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<1, true>(lhs, rhs).map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F32MulRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<2, true>(lhs, rhs).map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F32DivRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<3, true>(lhs, rhs).map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F32RemRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            payload::float_binary::<4, true>(lhs, rhs).map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F64AddRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::float_binary::<0, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F64SubRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::float_binary::<1, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F64MulRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::float_binary::<2, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F64DivRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::float_binary::<3, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F64RemRK { dst, lhs, rhs } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::float_binary::<4, false>(lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8ShlRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 8, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8ShrRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 8, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16ShlRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 16, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16ShrRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 16, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32ShlRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 32, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32ShrRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 32, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64ShlRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 64, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64ShrRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 64, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8ShlRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 8, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8ShrRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 8, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16ShlRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 16, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16ShrRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 16, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32ShlRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 32, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32ShrRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 32, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64ShlRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 64, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64ShrRR {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 64, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8ShlRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 8, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I8ShrRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 8, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16ShlRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 16, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16ShrRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 16, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32ShlRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 32, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32ShrRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 32, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64ShlRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 64, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64ShrRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 64, true>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8ShlRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 8, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8ShrRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 8, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16ShlRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 16, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16ShrRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 16, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32ShlRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 32, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32ShrRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 32, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64ShlRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<0, 64, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64ShrRK {
                    dst,
                    lhs,
                    rhs,
                    count_domain,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            payload::shift_value::<1, 64, false>(lhs, rhs, count_domain)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32CompareRR {
                    dst,
                    lhs,
                    rhs,
                    condition,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            comparison::i32_compare(lhs, rhs, condition)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64CompareRR {
                    dst,
                    lhs,
                    rhs,
                    condition,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            comparison::i64_compare(lhs, rhs, condition)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64CompareRR {
                    dst,
                    lhs,
                    rhs,
                    condition,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            comparison::u64_compare(lhs, rhs, condition)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F32CompareRR {
                    dst,
                    lhs,
                    rhs,
                    condition,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            comparison::f32_compare(lhs, rhs, condition)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F64CompareRR {
                    dst,
                    lhs,
                    rhs,
                    condition,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            comparison::f64_compare(lhs, rhs, condition)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32CompareRK {
                    dst,
                    lhs,
                    rhs,
                    condition,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            comparison::i32_compare(lhs, rhs, condition)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64CompareRK {
                    dst,
                    lhs,
                    rhs,
                    condition,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as u64;
                    (
                        dst,
                        attempt!(
                            comparison::i64_compare(lhs, rhs, condition)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64CompareRK {
                    dst,
                    lhs,
                    rhs,
                    condition,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            comparison::u64_compare(lhs, rhs, condition)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F32CompareRK {
                    dst,
                    lhs,
                    rhs,
                    condition,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    (
                        dst,
                        attempt!(
                            comparison::f32_compare(lhs, rhs, condition)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F64CompareRK {
                    dst,
                    lhs,
                    rhs,
                    condition,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    (
                        dst,
                        attempt!(
                            comparison::f64_compare(lhs, rhs, condition)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::CharCompareRR {
                    dst,
                    lhs,
                    rhs,
                    condition,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            comparison::char_compare(lhs, rhs, condition)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::ScalarCompareRR {
                    dst,
                    lhs,
                    rhs,
                    condition,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            comparison::scalar_compare(lhs, rhs, condition)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32CompareBranchRR {
                    lhs,
                    rhs,
                    condition,
                    then_target,
                    else_target,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let value = attempt!(
                        comparison::i32_compare(lhs, rhs, condition).map_err(ScalarFailure::Trap)
                    );
                    next = attempt!(self.jump(if value == 1 {
                        then_target.index()
                    } else {
                        else_target.index()
                    }));
                    continue;
                }
                ExecutionInstruction::I64CompareBranchRR {
                    lhs,
                    rhs,
                    condition,
                    then_target,
                    else_target,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let value = attempt!(
                        comparison::i64_compare(lhs, rhs, condition).map_err(ScalarFailure::Trap)
                    );
                    next = attempt!(self.jump(if value == 1 {
                        then_target.index()
                    } else {
                        else_target.index()
                    }));
                    continue;
                }
                ExecutionInstruction::U64CompareBranchRR {
                    lhs,
                    rhs,
                    condition,
                    then_target,
                    else_target,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let value = attempt!(
                        comparison::u64_compare(lhs, rhs, condition).map_err(ScalarFailure::Trap)
                    );
                    next = attempt!(self.jump(if value == 1 {
                        then_target.index()
                    } else {
                        else_target.index()
                    }));
                    continue;
                }
                ExecutionInstruction::F32CompareBranchRR {
                    lhs,
                    rhs,
                    condition,
                    then_target,
                    else_target,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let value = attempt!(
                        comparison::f32_compare(lhs, rhs, condition).map_err(ScalarFailure::Trap)
                    );
                    next = attempt!(self.jump(if value == 1 {
                        then_target.index()
                    } else {
                        else_target.index()
                    }));
                    continue;
                }
                ExecutionInstruction::F64CompareBranchRR {
                    lhs,
                    rhs,
                    condition,
                    then_target,
                    else_target,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let value = attempt!(
                        comparison::f64_compare(lhs, rhs, condition).map_err(ScalarFailure::Trap)
                    );
                    next = attempt!(self.jump(if value == 1 {
                        then_target.index()
                    } else {
                        else_target.index()
                    }));
                    continue;
                }
                ExecutionInstruction::I32CompareBranchRK {
                    lhs,
                    rhs,
                    condition,
                    then_target,
                    else_target,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    let value = attempt!(
                        comparison::i32_compare(lhs, rhs, condition).map_err(ScalarFailure::Trap)
                    );
                    next = attempt!(self.jump(if value == 1 {
                        then_target.index()
                    } else {
                        else_target.index()
                    }));
                    continue;
                }
                ExecutionInstruction::I64CompareBranchRK {
                    lhs,
                    rhs,
                    condition,
                    then_target,
                    else_target,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as u64;
                    let value = attempt!(
                        comparison::i64_compare(lhs, rhs, condition).map_err(ScalarFailure::Trap)
                    );
                    next = attempt!(self.jump(if value == 1 {
                        then_target.index()
                    } else {
                        else_target.index()
                    }));
                    continue;
                }
                ExecutionInstruction::U64CompareBranchRK {
                    lhs,
                    rhs,
                    condition,
                    then_target,
                    else_target,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    let value = attempt!(
                        comparison::u64_compare(lhs, rhs, condition).map_err(ScalarFailure::Trap)
                    );
                    next = attempt!(self.jump(if value == 1 {
                        then_target.index()
                    } else {
                        else_target.index()
                    }));
                    continue;
                }
                ExecutionInstruction::F32CompareBranchRK {
                    lhs,
                    rhs,
                    condition,
                    then_target,
                    else_target,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = rhs as i64 as u64;
                    let value = attempt!(
                        comparison::f32_compare(lhs, rhs, condition).map_err(ScalarFailure::Trap)
                    );
                    next = attempt!(self.jump(if value == 1 {
                        then_target.index()
                    } else {
                        else_target.index()
                    }));
                    continue;
                }
                ExecutionInstruction::F64CompareBranchRK {
                    lhs,
                    rhs,
                    condition,
                    then_target,
                    else_target,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));

                    let value = attempt!(
                        comparison::f64_compare(lhs, rhs, condition).map_err(ScalarFailure::Trap)
                    );
                    next = attempt!(self.jump(if value == 1 {
                        then_target.index()
                    } else {
                        else_target.index()
                    }));
                    continue;
                }
                ExecutionInstruction::CharCompareBranchRR {
                    lhs,
                    rhs,
                    condition,
                    then_target,
                    else_target,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let value = attempt!(
                        comparison::char_compare(lhs, rhs, condition).map_err(ScalarFailure::Trap)
                    );
                    next = attempt!(self.jump(if value == 1 {
                        then_target.index()
                    } else {
                        else_target.index()
                    }));
                    continue;
                }
                ExecutionInstruction::ScalarCompareBranchRR {
                    lhs,
                    rhs,
                    condition,
                    then_target,
                    else_target,
                } => {
                    let lhs = attempt!(self.payload(lhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let value = attempt!(
                        comparison::scalar_compare(lhs, rhs, condition)
                            .map_err(ScalarFailure::Trap)
                    );
                    next = attempt!(self.jump(if value == 1 {
                        then_target.index()
                    } else {
                        else_target.index()
                    }));
                    continue;
                }
                ExecutionInstruction::I8BitNotR { dst, src } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<8, 8, true>(value, value)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I16BitNotR { dst, src } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<8, 16, true>(value, value)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32BitNotR { dst, src } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<8, 32, true>(value, value)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64BitNotR { dst, src } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<8, 64, true>(value, value)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U8BitNotR { dst, src } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<8, 8, false>(value, value)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U16BitNotR { dst, src } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<8, 16, false>(value, value)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U32BitNotR { dst, src } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<8, 32, false>(value, value)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::U64BitNotR { dst, src } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::integer_value::<8, 64, false>(value, value)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I32NegR { dst, src } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::negate_integer::<32>(value, value)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::I64NegR { dst, src } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::negate_integer::<64>(value, value)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F32NegR { dst, src } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::negate_float::<true>(value, value)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::F64NegR { dst, src } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::negate_float::<false>(value, value)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::BoolNotR { dst, src } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(payload::boolean_not(value, value).map_err(ScalarFailure::Trap)),
                    )
                }
                ExecutionInstruction::IntegerConvert {
                    dst,
                    src,
                    source,
                    target,
                } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::convert(value, source.builtin_type(), target.builtin_type())
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::IntegerToFloat {
                    dst,
                    src,
                    source,
                    target,
                } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::convert(value, source.builtin_type(), target.builtin_type())
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::FloatToInteger {
                    dst,
                    src,
                    source,
                    target,
                } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::convert(value, source.builtin_type(), target.builtin_type())
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::FloatConvert {
                    dst,
                    src,
                    source,
                    target,
                } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::convert(value, source.builtin_type(), target.builtin_type())
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                // END GENERATED scalar-execution
                ExecutionInstruction::BoolToInteger { dst, src, target } => {
                    let value =
                        attempt!(self.payload(src).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            payload::convert(value, BuiltinType::Bool, target.builtin_type())
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::IntegerBinaryKR {
                    dst,
                    lhs,
                    rhs,
                    op,
                    domain,
                } => {
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            shared::integer(op, domain, lhs, rhs).map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::IntegerBinaryKK {
                    dst,
                    lhs,
                    rhs,
                    op,
                    domain,
                } => (
                    dst,
                    attempt!(shared::integer(op, domain, lhs, rhs).map_err(ScalarFailure::Trap)),
                ),
                ExecutionInstruction::FloatBinaryKR {
                    dst,
                    lhs,
                    rhs,
                    op,
                    domain,
                } => {
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(shared::float(op, domain, lhs, rhs).map_err(ScalarFailure::Trap)),
                    )
                }
                ExecutionInstruction::FloatBinaryKK {
                    dst,
                    lhs,
                    rhs,
                    op,
                    domain,
                } => (
                    dst,
                    attempt!(shared::float(op, domain, lhs, rhs).map_err(ScalarFailure::Trap)),
                ),
                ExecutionInstruction::ShiftKR {
                    dst,
                    lhs,
                    rhs,
                    kind,
                    count_domain,
                } => {
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            shared::shift(kind, count_domain, lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::ShiftKK {
                    dst,
                    lhs,
                    rhs,
                    kind,
                    count_domain,
                } => (
                    dst,
                    attempt!(
                        shared::shift(kind, count_domain, lhs, rhs).map_err(ScalarFailure::Trap)
                    ),
                ),
                ExecutionInstruction::CompareStoreKR {
                    dst,
                    lhs,
                    rhs,
                    domain,
                    condition,
                } => {
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    (
                        dst,
                        attempt!(
                            domain
                                .evaluate(condition, lhs, rhs)
                                .map_err(ScalarFailure::Trap)
                        ),
                    )
                }
                ExecutionInstruction::CompareStoreKK {
                    dst,
                    lhs,
                    rhs,
                    domain,
                    condition,
                } => (
                    dst,
                    attempt!(
                        domain
                            .evaluate(condition, lhs, rhs)
                            .map_err(ScalarFailure::Trap)
                    ),
                ),
                ExecutionInstruction::CompareBranchKR {
                    lhs,
                    rhs,
                    domain,
                    condition,
                    then_target,
                    else_target,
                } => {
                    let rhs = attempt!(self.payload(rhs).ok_or(ScalarFailure::InvalidOperandSlot));
                    let value = attempt!(
                        domain
                            .evaluate(condition, lhs, rhs)
                            .map_err(ScalarFailure::Trap)
                    );
                    next = attempt!(self.jump(if value == 1 {
                        then_target.index()
                    } else {
                        else_target.index()
                    }));
                    continue;
                }
                ExecutionInstruction::Jump(target) => {
                    next = attempt!(self.jump(target.index()));
                    continue;
                }
                ExecutionInstruction::Branch {
                    cond,
                    then_target,
                    else_target,
                } => {
                    let target =
                        match attempt!(self.payload(cond).ok_or(ScalarFailure::InvalidOperandSlot))
                        {
                            1 => then_target,
                            0 => else_target,
                            _ => {
                                break 'segment Err(ScalarFailure::InvalidBranchCondition);
                            }
                        };
                    next = attempt!(self.jump(target.index()));
                    continue;
                }
                ExecutionInstruction::Return {
                    value,
                    representation,
                } => {
                    let value = match value {
                        Some(slot) => ReturnValue::scalar(
                            representation,
                            attempt!(self.payload(slot).ok_or(ScalarFailure::InvalidOperandSlot)),
                        ),
                        None => ReturnValue::scalar(representation, 0),
                    };
                    break 'segment Ok(ScalarExit::Return(value));
                }
                ExecutionInstruction::Managed(operation) => {
                    break 'segment Ok(ScalarExit::Managed(operation));
                }
                ExecutionInstruction::Boundary => break 'segment Ok(ScalarExit::Boundary),
            };
            attempt!(
                self.write_payload(dst, value)
                    .ok_or(ScalarFailure::InvalidOperandSlot)
            );
        };
        *self.ip = next;
        *self.executing = executing;
        result
    }
}
