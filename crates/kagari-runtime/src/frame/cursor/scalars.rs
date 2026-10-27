//! Borrow code and scalar banks once while preserving each logical boundary.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::{cursor::ExecutionCursor, transfer::ReturnValue},
    module::execution::{ExecutionInstruction, ScalarSlot, managed::PreparedManagedOperation},
};
use kagari_types::{
    payload::{self, comparison, shared},
    scalar::BuiltinType,
};

pub(super) enum ScalarExit {
    Slice,
    Safepoint,
    Boundary,
    Return(ReturnValue),
    Managed(PreparedManagedOperation),
}

/// Defer owned diagnostics until the closed segment has stopped executing.
enum ScalarFailure {
    Trap(&'static str),
    Invariant(&'static str),
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
        .map_err(|failure| self.scalar_error(failure))
    }

    #[inline(always)]
    pub(super) fn payload(&self, slot: ScalarSlot) -> Option<u64> {
        if !self
            .values
            .initialized
            .get(slot.index())
            .copied()
            .unwrap_or(false)
        {
            return None;
        }
        self.values.payloads.get(slot.index()).copied()
    }

    pub(super) fn write_payload(&mut self, slot: ScalarSlot, value: u64) -> Option<()> {
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

    fn jump(&mut self, target: usize) -> Result<(), ScalarFailure> {
        if target >= self.function.instructions.len() {
            return Err(ScalarFailure::Invariant("invalid frame jump target"));
        }
        *self.ip = target;
        Ok(())
    }

    #[cold]
    fn scalar_error(&self, failure: ScalarFailure) -> RuntimeError {
        match failure {
            ScalarFailure::Trap(reason) => RuntimeError::new(RuntimeErrorKind::ScriptTrap, reason),
            ScalarFailure::Invariant(reason) => self.runtime.resources().quarantine(reason),
        }
    }
}

macro_rules! scalar_loop {
    (binary { $(($bp:pat, $brr:ident, $brk:ident, $bt:ty, $be:path),)* }
     shift { $(($sk:ident, $srr:ident, $srk:ident, $se:path),)* }
     compare { $(($cd:ident, $crr:ident, $crk:ident, $jrr:ident, $jrk:ident, $ct:ty, $ce:path),)* }
     simple_compare { $(($sd:ident, $qrr:ident, $qjrr:ident, $qe:path),)* }
     unary { $(($u:ident, $ur:ident, $ue:path),)* }
     convert { $(($cv:ident, $cs:ty, $ctarget:ty),)* }) => {
        impl<'code> ExecutionCursor<'code> {
            // Keep object handlers out of this loop's register allocation and inlining
            // budget. An unbounded segment has no optional countdown to load or spill.
            #[inline(never)]
            fn execute_slice<S: InstructionSlice>(
                &mut self,
                slice: &mut S,
                collection_due: bool,
                mut first: bool,
            ) -> Result<ScalarExit, ScalarFailure> {
                loop {
                    if !first && slice.exhausted() {
                        return Ok(ScalarExit::Slice);
                    }
                    if !first && self.prepare_instruction(collection_due) {
                        return Ok(ScalarExit::Safepoint);
                    }
                    first = false;
                    slice.advance();
                    let instruction = self.next_instruction()
                        .ok_or(ScalarFailure::Invariant("verified function fell through"))?;
                    let (dst, value) = match *instruction {
                        ExecutionInstruction::Constant { dst, value } => (dst, value),
                        ExecutionInstruction::Move { dst, src } => (dst, self.payload(src).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?),
                        $(ExecutionInstruction::$brr { dst, lhs, rhs } => {
                            let lhs = self.payload(lhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            let rhs = self.payload(rhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            (dst, $be(lhs, rhs).map_err(ScalarFailure::Trap)?)
                        },)*
                        $(ExecutionInstruction::$brk { dst, lhs, rhs } => {
                            let lhs = self.payload(lhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            let rhs = rhs as i64 as u64;
                            (dst, $be(lhs, rhs).map_err(ScalarFailure::Trap)?)
                        },)*
                        $(ExecutionInstruction::$srr { dst, lhs, rhs, count_domain } => {
                            let lhs = self.payload(lhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            let rhs = self.payload(rhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            (dst, $se(lhs, rhs, count_domain).map_err(ScalarFailure::Trap)?)
                        },)*
                        $(ExecutionInstruction::$srk { dst, lhs, rhs, count_domain } => {
                            let lhs = self.payload(lhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            let rhs = rhs as i64 as u64;
                            (dst, $se(lhs, rhs, count_domain).map_err(ScalarFailure::Trap)?)
                        },)*
                        $(ExecutionInstruction::$crr { dst, lhs, rhs, condition } => {
                            let lhs = self.payload(lhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            let rhs = self.payload(rhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            (dst, $ce(lhs, rhs, condition).map_err(ScalarFailure::Trap)?)
                        },)*
                        $(ExecutionInstruction::$crk { dst, lhs, rhs, condition } => {
                            let lhs = self.payload(lhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            let rhs = rhs as i64 as u64;
                            (dst, $ce(lhs, rhs, condition).map_err(ScalarFailure::Trap)?)
                        },)*
                        $(ExecutionInstruction::$qrr { dst, lhs, rhs, condition } => {
                            let lhs = self.payload(lhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            let rhs = self.payload(rhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            (dst, $qe(lhs, rhs, condition).map_err(ScalarFailure::Trap)?)
                        },)*
                        $(ExecutionInstruction::$jrr { lhs, rhs, condition, then_target, else_target } => {
                            let lhs = self.payload(lhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            let rhs = self.payload(rhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            let value = $ce(lhs, rhs, condition).map_err(ScalarFailure::Trap)?;
                            self.jump(if value == 1 { then_target.index() } else { else_target.index() })?;
                            continue;
                        },)*
                        $(ExecutionInstruction::$jrk { lhs, rhs, condition, then_target, else_target } => {
                            let lhs = self.payload(lhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            let rhs = rhs as i64 as u64;
                            let value = $ce(lhs, rhs, condition).map_err(ScalarFailure::Trap)?;
                            self.jump(if value == 1 { then_target.index() } else { else_target.index() })?;
                            continue;
                        },)*
                        $(ExecutionInstruction::$qjrr { lhs, rhs, condition, then_target, else_target } => {
                            let lhs = self.payload(lhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            let rhs = self.payload(rhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            let value = $qe(lhs, rhs, condition).map_err(ScalarFailure::Trap)?;
                            self.jump(if value == 1 { then_target.index() } else { else_target.index() })?;
                            continue;
                        },)*
                        $(ExecutionInstruction::$ur { dst, src } => {
                            let value = self.payload(src).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            (dst, $ue(value, value).map_err(ScalarFailure::Trap)?)
                        },)*
                        $(ExecutionInstruction::$cv { dst, src, source, target } => {
                            let value = self.payload(src).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            (dst, payload::convert(value, source.builtin_type(), target.builtin_type()).map_err(ScalarFailure::Trap)?)
                        },)*
                        ExecutionInstruction::BoolToInteger { dst, src, target } => {
                            let value = self.payload(src).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            (dst, payload::convert(value, BuiltinType::Bool, target.builtin_type()).map_err(ScalarFailure::Trap)?)
                        }
                        ExecutionInstruction::IntegerBinaryKR { dst, lhs, rhs, op, domain } => {
                            let rhs = self.payload(rhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            (dst, shared::integer(op, domain, lhs, rhs).map_err(ScalarFailure::Trap)?)
                        }
                        ExecutionInstruction::IntegerBinaryKK { dst, lhs, rhs, op, domain } => {
                            (dst, shared::integer(op, domain, lhs, rhs).map_err(ScalarFailure::Trap)?)
                        }
                        ExecutionInstruction::FloatBinaryKR { dst, lhs, rhs, op, domain } => {
                            let rhs = self.payload(rhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            (dst, shared::float(op, domain, lhs, rhs).map_err(ScalarFailure::Trap)?)
                        }
                        ExecutionInstruction::FloatBinaryKK { dst, lhs, rhs, op, domain } => {
                            (dst, shared::float(op, domain, lhs, rhs).map_err(ScalarFailure::Trap)?)
                        }
                        ExecutionInstruction::ShiftKR { dst, lhs, rhs, kind, count_domain } => {
                            let rhs = self.payload(rhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            (dst, shared::shift(kind, count_domain, lhs, rhs).map_err(ScalarFailure::Trap)?)
                        }
                        ExecutionInstruction::ShiftKK { dst, lhs, rhs, kind, count_domain } => {
                            (dst, shared::shift(kind, count_domain, lhs, rhs).map_err(ScalarFailure::Trap)?)
                        }
                        ExecutionInstruction::CompareStoreKR { dst, lhs, rhs, domain, condition } => {
                            let rhs = self.payload(rhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            (dst, domain.evaluate(condition, lhs, rhs).map_err(ScalarFailure::Trap)?)
                        }
                        ExecutionInstruction::CompareStoreKK { dst, lhs, rhs, domain, condition } => {
                            (dst, domain.evaluate(condition, lhs, rhs).map_err(ScalarFailure::Trap)?)
                        }
                        ExecutionInstruction::CompareBranchKR { lhs, rhs, domain, condition, then_target, else_target } => {
                            let rhs = self.payload(rhs).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                            let value = domain.evaluate(condition, lhs, rhs).map_err(ScalarFailure::Trap)?;
                            self.jump(if value == 1 { then_target.index() } else { else_target.index() })?;
                            continue;
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
                            let target = match self.payload(cond).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))? {
                                1 => then_target,
                                0 => else_target,
                                _ => {
                                    return Err(ScalarFailure::Invariant("verified branch condition is not bool"));
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
                                Some(slot) => ReturnValue::scalar(representation, self.payload(slot).ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?),
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
                        .ok_or(ScalarFailure::Invariant("invalid execution operand slot"))?;
                }
            }

        }
    };
}

scalar_catalog!(scalar_loop);
