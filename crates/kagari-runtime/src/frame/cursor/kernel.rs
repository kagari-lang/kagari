//! Closed operations reuse authority without allocating or admitting callbacks.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    error::RuntimeError,
    frame::{
        cursor::{ExecutionCursor, primitives::NativeContinuation, scalars::ScalarExit},
        transfer::ReturnValue,
    },
    module::execution::{
        layout::Location,
        managed::{ManagedOperation, PreparedManagedOperation},
    },
};

use kagari_bytecode::instruction::ConstantId;

pub enum RegionExit {
    Slice,
    Safepoint,
    Boundary,
    Return(ReturnValue),
}

#[derive(Debug, thiserror::Error)]
pub enum RegionError {
    #[error(transparent)]
    // Detailed runtime failures are cold, owned data. Keep their message/trace
    // payload out of every successful region return and managed-operation result.
    Runtime(Box<RuntimeError>),
    #[error("{0}")]
    TypeMismatch(&'static str),
    #[error("invalid index: {0}")]
    InvalidIndex(usize),
}

impl From<RuntimeError> for RegionError {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(Box::new(error))
    }
}

pub(super) enum CursorExit {
    Region(RegionExit),
    Transition(PreparedTransition),
}

pub(super) enum PreparedTransition {
    Constant { dst: Location, constant: ConstantId },
    Field { index: usize },
    TupleWrite { operation: usize, index: usize },
}

impl ExecutionCursor<'_> {
    /// Execute sealed nonallocating operations under one authority check. The driver
    /// has already polled and observed the first PC; subsequent logical PCs keep
    /// the same cancellation, collection and observation boundaries. No caller
    /// supplies values or callbacks while authority is reused.
    pub(super) fn execute_region(
        &mut self,
        remaining: &mut Option<usize>,
    ) -> Result<CursorExit, RegionError> {
        // A closed region cannot allocate heap records, mutate executable metadata
        // or change collector policy. Field operations copy rooted Values using
        // checked storage methods; no destructor/callback runs on replacement.
        // Recompute after every allocating or reentrant boundary.
        // Candidate leases can expire on another thread. Their collection request
        // is polled at each logical PC, as are cancellation and observer requests.
        let collection_due = self.runtime.gc().collection_due();
        let mut first = true;
        loop {
            match self.scalars()?.execute(remaining, collection_due, first)? {
                ScalarExit::Slice => return Ok(CursorExit::Region(RegionExit::Slice)),
                ScalarExit::Safepoint => return Ok(CursorExit::Region(RegionExit::Safepoint)),
                ScalarExit::Return(value) => {
                    return Ok(CursorExit::Region(RegionExit::Return(value)));
                }
                ScalarExit::Boundary => {
                    #[cfg(feature = "execution-diagnostics")]
                    diagnostics::record(Event::SlowBoundary);
                    return Ok(CursorExit::Region(RegionExit::Boundary));
                }
                ScalarExit::Managed(PreparedManagedOperation(ManagedOperation::Constant {
                    dst,
                    constant,
                })) => {
                    let pool = &self
                        .frame
                        .links
                        .as_ref()
                        .ok_or_else(|| self.invalid())?
                        .constants;
                    let Some(value) = pool.get(constant) else {
                        return Ok(CursorExit::Transition(PreparedTransition::Constant {
                            dst,
                            constant,
                        }));
                    };
                    if !self.runtime.gc().validate_value(&value) {
                        return Err(self.invalid().into());
                    }
                    self.values
                        .write_location(dst, value)
                        .ok_or_else(|| self.invalid())?;
                }
                ScalarExit::Managed(PreparedManagedOperation(ManagedOperation::Copy {
                    dst,
                    src,
                })) => {
                    // The admitted banks own both roots. Keep the ordinary heap
                    // identity and destination representation checks; copying a
                    // Value cannot allocate, collect, run a destructor or reenter.
                    let value = self
                        .values
                        .read_location(src)
                        .ok_or_else(|| self.invalid())?;
                    if !self.runtime.gc().validate_value(&value) {
                        return Err(self.invalid().into());
                    }
                    self.values
                        .write_location(dst, value)
                        .ok_or_else(|| self.invalid())?;
                }
                ScalarExit::Managed(PreparedManagedOperation(ManagedOperation::Return {
                    value,
                })) => {
                    // The callee window retains this value through adaptation.
                    // Common retirement/publication permits no GC or callback gap.
                    let value = self
                        .values
                        .managed
                        .get(value.index())
                        .copied()
                        .ok_or_else(|| self.invalid())?;
                    return Ok(CursorExit::Region(RegionExit::Return(
                        ReturnValue::general(value),
                    )));
                }
                ScalarExit::Managed(PreparedManagedOperation(ManagedOperation::Field {
                    index,
                })) => {
                    if let Some(exit) = self.execute_field(index)? {
                        return Ok(exit);
                    }
                }
                ScalarExit::Managed(PreparedManagedOperation(ManagedOperation::Index {
                    index,
                })) => {
                    if let Some(exit) = self.execute_index(index)? {
                        return Ok(exit);
                    }
                }
                ScalarExit::Managed(PreparedManagedOperation(ManagedOperation::Native {
                    index,
                })) => {
                    if let NativeContinuation::Boundary = self.execute_native(index)? {
                        return Ok(CursorExit::Region(RegionExit::Boundary));
                    }
                }
            }
            // The operation's PC and slice unit were consumed before the handoff.
            // Its successor still needs the normal logical boundary checks.
            first = false;
        }
    }
}
