//! Frames name authoritative source banks and own one linear suffix mark.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
pub(crate) mod operands;
#[cfg(test)]
mod tests;
use crate::{
    error::RuntimeError,
    execution::{
        memory::ManagedMemory,
        roots::{
            DependencyId, SessionId, Slots, ValueBank, ValueLocation,
            frames::{FrameBanks, FrameMark},
        },
    },
    frame::{
        arguments::{ArgumentError, FrameArguments},
        values::operands::{OperandWindow, read_operand, write_operand},
    },
    module::{
        execution::layout::{FrameLayout, Location},
        managed::revision::RevisionState,
    },
    resource::ResourceState,
    value::{
        carriers::ScopedValue,
        kind::ObjectKind,
        scalar::{self, domain},
        view::ValueView,
    },
};
use kagari_gc::{
    error::GcError,
    object::{AnyObject, Gc},
};

#[derive(Debug)]
pub(crate) enum FrameAllocationError {
    Storage(GcError),
    Execution(RuntimeError),
}

impl From<ArgumentError> for FrameAllocationError {
    fn from(error: ArgumentError) -> Self {
        match error {
            ArgumentError::Input(error) => Self::Execution(error),
            ArgumentError::Storage(error) => Self::Storage(error),
        }
    }
}

impl From<GcError> for FrameAllocationError {
    fn from(error: GcError) -> Self {
        Self::Storage(error)
    }
}

impl From<RuntimeError> for FrameAllocationError {
    fn from(error: RuntimeError) -> Self {
        Self::Execution(error)
    }
}

/// Semantic admission supplies every actual executable relationship. Immutable
/// descriptors/old metadata IDs alone are not dependencies. The frame owns copied
/// edges; a synchronous same-execution body may borrow its admitted caller prefix.
pub(crate) struct FrameMetadata<'edges, 'gc> {
    pub(crate) body: Gc<'gc, RevisionState>,
    pub(crate) inherited_body: Option<DependencyId<RevisionState>>,
    pub(crate) dependencies: &'edges [Gc<'gc, AnyObject>],
}

/// Non-owning coordinates survive backing growth. No independent window registry
/// or per-frame generation exists; the frame's mark and Rust call lifetime own
/// ordinary access, and the complete execution ID admits cross-activation use.
#[derive(Clone, Copy)]
pub(crate) struct FrameSlots {
    pub(crate) banks: FrameBanks,
    pub(crate) body: DependencyId<RevisionState>,
}

/// The control frame stores its mark once. Borrowed slot views derive coordinates
/// from it instead of duplicating bank ranges in a separate window directory.
pub(crate) struct FrameStorage {
    mark: FrameMark,
    body: DependencyId<RevisionState>,
}

impl FrameStorage {
    pub(super) fn mark(&self) -> &FrameMark {
        &self.mark
    }

    pub(super) fn mark_mut(&mut self) -> &mut FrameMark {
        &mut self.mark
    }

    pub(crate) fn slots(&self) -> FrameSlots {
        FrameSlots {
            banks: self.mark.banks(),
            body: self.body,
        }
    }

    pub(crate) fn check_exit(&self, slots: &mut Slots<'_, '_>) -> Result<(), GcError> {
        slots.check_frame_exit(&self.mark)
    }

    pub(crate) fn retire(self, slots: &mut Slots<'_, '_>) -> Result<(), GcError> {
        slots.retire_frame(self.mark)
    }
}

impl FrameSlots {
    /// A cold, pointer-free observation. Its owner keeps these coordinates alive;
    /// it never opens a heap scope or retains a Host root for a read.
    pub(crate) fn inspect<'view>(
        self,
        memory: &ManagedMemory,
        location: Location,
    ) -> Result<ValueView<'view>, GcError> {
        if !memory.contains_frame_banks(self.banks) {
            return Err(GcError::Index);
        }
        if location.operand.is_unit() {
            return location
                .admits(ScopedValue::unit())
                .then_some(ValueView::unit())
                .ok_or(GcError::Representation);
        }
        if location.operand.managed() {
            let value = memory.inspect_value(self.managed_location(location)?)?;
            let borrowed = location.borrowed_receiver
                && matches!(
                    value.kind(),
                    Some(ObjectKind::Ephemeral | ObjectKind::HostPath)
                );
            if !value.is_unit() && !borrowed && !value.has_representation(location.representation) {
                return Err(GcError::Representation);
            }
            return Ok(value);
        }
        if location.operand.index() >= self.banks.scalar_count {
            return Err(GcError::Index);
        }
        let Some(bits) = memory.inspect_scalar(
            self.banks.execution,
            self.banks.scalars + location.operand.index(),
        )?
        else {
            return Ok(ValueView::unit());
        };
        if !domain::admits_payload(location.representation, location.semantic, bits) {
            return Err(GcError::Representation);
        }
        ValueView::protected(
            scalar::decode(location.representation, bits).ok_or(GcError::Representation)?,
            None,
        )
    }

    pub(crate) fn allocate<'gc>(
        slots: &mut Slots<'_, 'gc>,
        execution: SessionId,
        layout: &FrameLayout,
        parameters: &[usize],
        arguments: FrameArguments<'_, '_>,
        metadata: FrameMetadata<'_, 'gc>,
        resources: &ResourceState,
    ) -> Result<FrameStorage, FrameAllocationError> {
        if parameters.len() != arguments.len() {
            return Err(GcError::Representation.into());
        }
        arguments.check_bounds(slots)?;
        // Full original admission precedes all growth. A later stale coordinate
        // must not become valid because this frame reserved the same tail index.
        for (index, &parameter) in parameters.iter().enumerate() {
            if index != 0 && index.is_multiple_of(1024) {
                resources.poll_closed()?;
            }
            let location = layout.location(parameter).ok_or(GcError::Index)?;
            let value = arguments.get(slots, index)?;
            if !fits(layout, location) {
                return Err(GcError::Representation.into());
            }
            if !location.admits(value) {
                return Err(arguments.reject(index, GcError::Representation).into());
            }
        }
        let inherited = match metadata.inherited_body {
            Some(body) => {
                if !slots.dependency(body)?.ptr_eq(metadata.body) {
                    return Err(GcError::Representation.into());
                }
                (body.execution() == execution).then_some(body)
            }
            None => None,
        };
        let mark =
            slots.enter_frame(execution, layout.managed_count, layout.scalar_count, || {
                resources.poll_closed().map_err(FrameAllocationError::from)
            })?;
        let banks = mark.banks();
        let publish: Result<Self, FrameAllocationError> = (|| {
            let body = match inherited {
                Some(body) => body,
                None => slots.push_dependency(execution, metadata.body)?,
            };
            for (index, dependency) in metadata.dependencies.iter().enumerate() {
                if index != 0 && index.is_multiple_of(1024) {
                    resources.poll_closed()?;
                }
                slots.push_dependency(execution, *dependency)?;
            }
            let frame = Self { banks, body };
            for (index, &parameter) in parameters.iter().enumerate() {
                if index != 0 && index.is_multiple_of(1024) {
                    resources.poll_closed()?;
                }
                let location = layout
                    .location(parameter)
                    .expect("admitted frame parameter");
                let value = arguments.get(slots, index)?;
                write_operand(slots, banks, location, value)?;
                #[cfg(feature = "execution-diagnostics")]
                diagnostics::record(Event::ArgumentTransfer);
            }
            Ok(frame)
        })();
        match publish {
            Ok(frame) => Ok(FrameStorage {
                mark,
                body: frame.body,
            }),
            Err(error) => {
                slots.retire_frame(mark)?;
                Err(error)
            }
        }
    }

    pub(crate) fn read<'gc>(
        self,
        slots: &mut Slots<'_, 'gc>,
        location: Location,
    ) -> Result<ScopedValue<'gc>, GcError> {
        read_operand(slots, self.banks, location)
    }

    pub(crate) fn view<'frame>(
        self,
        slots: &mut Slots<'_, '_>,
        location: Location,
    ) -> Result<ValueView<'frame>, GcError> {
        let value = self.read(slots, location)?;
        let source = if value.object().is_some() {
            Some(self.managed_location(location)?)
        } else {
            None
        };
        ValueView::protected(value, source)
    }

    pub(crate) fn write<'gc>(
        self,
        slots: &mut Slots<'_, 'gc>,
        location: Location,
        value: ScopedValue<'gc>,
    ) -> Result<(), GcError> {
        write_operand(slots, self.banks, location, value)
    }

    pub(crate) fn operands<'access, 'source, 'gc>(
        self,
        slots: &'access mut Slots<'source, 'gc>,
    ) -> Result<OperandWindow<'access, 'source, 'gc>, GcError> {
        slots.check_frame_banks(self.banks)?;
        Ok(OperandWindow {
            slots,
            banks: self.banks,
        })
    }

    pub(crate) fn managed_location(self, location: Location) -> Result<ValueLocation, GcError> {
        if !location.operand.managed() || location.operand.index() >= self.banks.managed_count {
            return Err(GcError::Index);
        }
        Ok(ValueLocation {
            execution: self.banks.execution,
            bank: ValueBank::Managed,
            slot: self
                .banks
                .managed
                .checked_add(location.operand.index())
                .ok_or(GcError::Index)?,
        })
    }

    /// Clear dead managed operands in place. The checked liveness bitmap belongs
    /// to this frame's prepared instruction; removing edges needs no new roots.
    pub(crate) fn retain_managed(
        self,
        slots: &mut Slots<'_, '_>,
        retained: &[u64],
        resources: &ResourceState,
    ) -> Result<(), FrameAllocationError> {
        slots.check_frame_banks(self.banks)?;
        for index in 0..self.banks.managed_count {
            if index.is_multiple_of(1024) {
                resources.poll_closed()?;
            }
            if retained
                .get(index / 64)
                .is_none_or(|bits| bits & (1 << (index % 64)) == 0)
            {
                slots.set(
                    ValueLocation {
                        execution: self.banks.execution,
                        bank: ValueBank::Managed,
                        slot: self.banks.managed + index,
                    },
                    ScopedValue::unit(),
                )?;
            }
        }
        Ok(())
    }
}

fn fits(layout: &FrameLayout, location: Location) -> bool {
    location.operand.is_unit()
        || location.operand.index()
            < if location.operand.managed() {
                layout.managed_count
            } else {
                layout.scalar_count
            }
}
