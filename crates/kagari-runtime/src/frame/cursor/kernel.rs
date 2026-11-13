//! Closed operations reuse authority without allocating or admitting callbacks.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    error::{RuntimeError, fault::EngineFault, reason::CancellationReason},
    frame::{
        cursor::{
            ExecutionCursor, objects::FieldContinuation, primitives::NativeContinuation,
            scalars::ScalarExit,
        },
        driver::ExecutionAction,
        transfer::ReturnValue,
    },
    module::execution::{
        layout::Location,
        managed::{ManagedOperation, PreparedManagedOperation},
    },
    module::managed::revision::RevisionState,
    value_check::scoped::CheckError,
};

use kagari_bytecode::instruction::ConstantId;
use kagari_gc::error::GcError;

pub enum RegionExit {
    Slice,
    Safepoint,
    Boundary,
    Call(ExecutionAction),
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
    InvalidIndex(u64),
}

impl From<RuntimeError> for RegionError {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(Box::new(error))
    }
}

impl From<CheckError> for RegionError {
    fn from(error: CheckError) -> Self {
        match error {
            CheckError::Storage(error) => Self::storage(error),
            CheckError::Execution(error) => error.into(),
        }
    }
}

impl RegionError {
    /// Defer Runtime mutation/trace capture until the closed source loan ends.
    pub(in crate::frame) fn storage(error: GcError) -> Self {
        match error {
            GcError::Allocation => RuntimeError::resource_limit("managed allocation"),
            GcError::Limit(name) => RuntimeError::resource_limit(name),
            GcError::Closed => RuntimeError::from(CancellationReason::RuntimeIsClosed),
            error => RuntimeError::from(EngineFault::ManagedCollectionFailed(error)),
        }
        .into()
    }
}

pub(super) enum CursorExit {
    Region(RegionExit),
    Transition(PreparedTransition),
}

pub(super) enum PreparedTransition {
    Call { index: usize },
    Upcast { index: usize },
    Native { index: usize },
    Constant { dst: Location, constant: ConstantId },
    Field { index: usize },
    TupleWrite { operation: usize, index: u64 },
}

impl ExecutionCursor<'_, '_, '_, '_> {
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
        let collection_due = self.collection_due;
        let mut first = true;
        loop {
            match self.execute_scalars(remaining, collection_due, first)? {
                ScalarExit::Slice => return Ok(CursorExit::Region(RegionExit::Slice)),
                ScalarExit::Safepoint => return Ok(CursorExit::Region(RegionExit::Safepoint)),
                ScalarExit::Return(value) => {
                    return Ok(CursorExit::Region(RegionExit::Return(value)));
                }
                ScalarExit::Managed(PreparedManagedOperation(ManagedOperation::Call { index })) => {
                    return Ok(CursorExit::Transition(PreparedTransition::Call { index }));
                }
                ScalarExit::Managed(PreparedManagedOperation(ManagedOperation::Upcast {
                    index,
                })) => {
                    return Ok(CursorExit::Transition(PreparedTransition::Upcast { index }));
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
                    let value = RevisionState::cached_constant(
                        self.values.scope(),
                        self.body,
                        self.member,
                        constant,
                    )
                    .map_err(RegionError::storage)?;
                    let Some(value) = value else {
                        return Ok(CursorExit::Transition(PreparedTransition::Constant {
                            dst,
                            constant,
                        }));
                    };
                    self.values
                        .write_location(dst, value)
                        .map_err(RegionError::storage)?;
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
                        .map_err(RegionError::storage)?;
                    self.values
                        .write_location(dst, value)
                        .map_err(RegionError::storage)?;
                }
                ScalarExit::Managed(PreparedManagedOperation(ManagedOperation::Return {
                    value,
                })) => {
                    // The callee window retains this value through adaptation.
                    // Common retirement/publication permits no GC or callback gap.
                    let value = self
                        .values
                        .read_managed(value)
                        .map_err(RegionError::storage)?;
                    return Ok(CursorExit::Region(RegionExit::Return(
                        self.values
                            .protect_return(value)
                            .map_err(RegionError::storage)?,
                    )));
                }
                ScalarExit::Managed(PreparedManagedOperation(ManagedOperation::Field {
                    index,
                })) => {
                    if let FieldContinuation::Prepare = self.execute_field(index)? {
                        return Ok(CursorExit::Transition(PreparedTransition::Field { index }));
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
                    if let NativeContinuation::Callback = self.execute_native(index)? {
                        return Ok(CursorExit::Transition(PreparedTransition::Native { index }));
                    }
                }
            }
            // The operation's PC and slice unit were consumed before the handoff.
            // Its successor still needs the normal logical boundary checks.
            first = false;
        }
    }
}
