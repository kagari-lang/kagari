//! One cold instruction authenticates sources and commits within its frame owner.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault, trap::TrapReason},
    execution::roots::{ExecutionRoots, Slots},
    frame::{FrameStack, types::compatibility::TypeView, values::FrameSlots},
    host::{HostBorrowTable, HostRegistry},
    module::execution::layout::Location,
    resource::ResourceState,
    value::{carriers::ScopedValue, properties::ValueProperties, view::ValueView},
    value_check::{
        data::candidate_access,
        scoped::{self, CheckError},
    },
};
use kagari_bytecode::instruction::Slot;
use kagari_gc::error::GcError;
use std::collections::TryReserveError;

const CHUNK: usize = 1024;

/// These coordinates have one instruction's lifetime. No callbacks, frame changes
/// or publication occur between admission and the final closed source operation.
pub(super) struct Inputs {
    frame: FrameSlots,
    fields: Locations,
    destination: Option<Location>,
}

pub(super) struct InstructionScope<'access, 'source, 'gc, 'runtime> {
    pub(super) slots: &'access mut Slots<'source, 'gc>,
    inputs: &'runtime Inputs,
    pub(super) host: &'runtime HostRegistry,
    pub(super) borrows: &'runtime HostBorrowTable,
    pub(super) resources: &'runtime ResourceState,
}

impl Inputs {
    pub(super) fn prepare(
        stack: &FrameStack,
        runtime: &Runtime,
        fields: &[Slot],
        destination: Slot,
    ) -> Result<Self, RuntimeError> {
        Self::prepare_inner(stack, runtime, fields, Some(destination))
    }

    pub(super) fn prepare_effect(
        stack: &FrameStack,
        runtime: &Runtime,
        fields: &[Slot],
    ) -> Result<Self, RuntimeError> {
        Self::prepare_inner(stack, runtime, fields, None)
    }

    fn prepare_inner(
        stack: &FrameStack,
        runtime: &Runtime,
        fields: &[Slot],
        destination: Option<Slot>,
    ) -> Result<Self, RuntimeError> {
        stack.validate_top(runtime)?;
        let result = (|| {
            let frame = stack.current(runtime)?;
            frame.validate_runtime(runtime)?;
            let storage = frame.slots();
            let mut locations = Locations::new(fields.len())
                .map_err(|_| RuntimeError::resource_limit("instruction input locations"))?;
            for (index, &field) in fields.iter().enumerate() {
                if index.is_multiple_of(CHUNK) {
                    runtime.resources.poll_closed()?;
                }
                let location = frame.slot_location(&runtime.resources, field, true)?;
                // Preserve input-before-layout admission without retaining a
                // copied Value pack or opening a source loan across preparation.
                storage.inspect(&runtime.memory, location).map_err(|_| {
                    runtime
                        .resources
                        .quarantine(EngineFault::InvalidFrameRegister)
                })?;
                locations.push(location);
            }
            Ok(Self {
                frame: storage,
                fields: locations,
                destination: destination
                    .map(|destination| frame.slot_location(&runtime.resources, destination, false))
                    .transpose()?,
            })
        })();
        result.map_err(|error| runtime.resources.admit_execution_error(error))
    }

    /// Copy nonowning source coordinates for an operation that will execute Host
    /// code. The Host scope snapshots actual values before invoking that code.
    pub(super) fn views<'frame>(
        &self,
        runtime: &Runtime,
    ) -> Result<Vec<ValueView<'frame>>, RuntimeError> {
        let mut values = Vec::new();
        values
            .try_reserve_exact(self.fields.len())
            .map_err(|_| RuntimeError::resource_limit("instruction operand views"))?;
        for index in 0..self.fields.len() {
            if index.is_multiple_of(CHUNK) {
                runtime.resources.poll_closed()?;
            }
            let location = self.fields.get(index).expect("admitted operand index");
            values.push(self.frame.inspect(&runtime.memory, location).map_err(|_| {
                runtime
                    .resources
                    .quarantine(EngineFault::InvalidFrameRegister)
            })?);
        }
        Ok(values)
    }

    pub(super) fn access<R>(
        &self,
        runtime: &mut Runtime,
        operation: impl for<'gc> FnOnce(&mut InstructionScope<'_, '_, 'gc, '_>) -> Result<R, CheckError>,
    ) -> Result<R, RuntimeError> {
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| {
                    resources.poll_closed()?;
                    operation(&mut InstructionScope {
                        slots,
                        inputs: self,
                        host,
                        borrows: host_borrows,
                        resources,
                    })
                })())
            })
        });
        result
            .map_err(|error| runtime.managed_memory_error(error))?
            .map_err(|error| {
                let error = error.into_runtime(runtime);
                runtime.resources.admit_execution_error(error)
            })
    }

    pub(super) fn write(
        self,
        stack: &FrameStack,
        runtime: &mut Runtime,
        construct: impl for<'gc> FnOnce(
            &mut InstructionScope<'_, '_, 'gc, '_>,
        ) -> Result<ScopedValue<'gc>, CheckError>,
    ) -> Result<(), RuntimeError> {
        let destination = self.destination.ok_or_else(|| {
            runtime.resources.quarantine(EngineFault::HostInvariant(
                "value instruction has no destination",
            ))
        })?;
        self.access(runtime, |cx| {
            let value = construct(cx)?;
            cx.resources.poll_closed()?;
            // Incoming publication and scalar/tag commit occur before this scope
            // ends. The frame already owns the result at the next GC safepoint.
            self.frame.write(cx.slots, destination, value)?;
            Ok(())
        })?;
        stack.complete_source_write(runtime);
        Ok(())
    }

    /// An effect handler owns its final precommit poll. There is no fallible
    /// postcommit admission that could obscure an already completed mutation.
    pub(super) fn effect(
        self,
        stack: &FrameStack,
        runtime: &mut Runtime,
        operation: impl for<'gc> FnOnce(
            &mut InstructionScope<'_, '_, 'gc, '_>,
        ) -> Result<(), CheckError>,
    ) -> Result<(), RuntimeError> {
        if self.destination.is_some() {
            return Err(runtime.resources.quarantine(EngineFault::HostInvariant(
                "effect instruction has a value destination",
            )));
        }
        self.access(runtime, operation)?;
        stack.complete_source_write(runtime);
        Ok(())
    }
}

impl<'gc> InstructionScope<'_, '_, 'gc, '_> {
    pub(super) fn len(&self) -> usize {
        self.inputs.fields.len()
    }

    pub(super) fn field(&mut self, index: usize) -> Result<ScopedValue<'gc>, CheckError> {
        if index.is_multiple_of(CHUNK) {
            self.resources.poll_closed()?;
        }
        let location = self.inputs.fields.get(index).ok_or(GcError::Index)?;
        let value = self.inputs.frame.read(self.slots, location)?;
        candidate_access(self.slots.scope(), value, self.resources)?;
        Ok(value)
    }

    /// Lend a source-backed operand only within this closed instruction. Scalar
    /// kernels read inline bits; a managed view never gains independent retention.
    pub(super) fn view(&mut self, index: usize) -> Result<ValueView<'gc>, CheckError> {
        let value = self.field(index)?;
        let location = self.inputs.fields.get(index).ok_or(GcError::Index)?;
        let source = if value.object().is_some() {
            Some(self.inputs.frame.managed_location(location)?)
        } else {
            None
        };
        Ok(ValueView::protected(value, source)?)
    }

    pub(super) fn fields(&mut self) -> Result<Vec<ScopedValue<'gc>>, CheckError> {
        let mut fields = Vec::new();
        fields.try_reserve_exact(self.inputs.fields.len())?;
        for index in 0..self.inputs.fields.len() {
            fields.push(self.field(index)?);
        }
        Ok(fields)
    }

    pub(super) fn check(
        &mut self,
        value: ScopedValue<'gc>,
        expected: TypeView<'_>,
        failure: TrapReason,
    ) -> Result<(), CheckError> {
        if !scoped::matches(
            self.slots.scope(),
            value,
            expected,
            self.host,
            self.borrows,
            self.resources,
        )? || !ValueProperties::of(self.slots.scope(), value)?.storable
        {
            return Err(RuntimeError::from(failure).into());
        }
        Ok(())
    }
}

/// Cell/range and static operations have at most two operands. Keep those
/// coordinates on the Rust stack instead of adding a Vec allocation per access.
/// Arbitrary aggregate arities retain one fallible instruction-local buffer.
enum Locations {
    Inline {
        values: [Option<Location>; 2],
        len: usize,
    },
    Many(Vec<Location>),
}

impl Locations {
    fn new(count: usize) -> Result<Self, TryReserveError> {
        if count <= 2 {
            Ok(Self::Inline {
                values: [None; 2],
                len: 0,
            })
        } else {
            let mut values = Vec::new();
            values.try_reserve_exact(count)?;
            Ok(Self::Many(values))
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Inline { len, .. } => *len,
            Self::Many(values) => values.len(),
        }
    }

    fn push(&mut self, value: Location) {
        match self {
            Self::Inline { values, len } => {
                values[*len] = Some(value);
                *len += 1;
            }
            Self::Many(values) => values.push(value),
        }
    }

    fn get(&self, index: usize) -> Option<Location> {
        match self {
            Self::Inline { values, .. } => values.get(index).copied().flatten(),
            Self::Many(values) => values.get(index).copied(),
        }
    }
}
