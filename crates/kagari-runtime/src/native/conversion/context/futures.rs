//! Cold construction and decoding keep actual captures and selected metadata alive.
use crate::{
    Runtime,
    error::{RuntimeError, trap::TrapReason},
    execution::roots::{ExecutionRoots, ValueBank},
    execution_metadata::managed::{
        ProtectedMetadata,
        sources::{MetadataError, MetadataSource},
    },
    frame::types::arguments::TypeArgument,
    module::{ExecutableModule, managed::source::RevisionSource},
    native::{
        binding::NativeResult,
        conversion::{
            arguments::ArgumentValues,
            context::{Admission, ConversionContext, ConversionLimits},
        },
        future::{
            ColdFuture,
            storage::{ColdObject, FuturePayload},
        },
    },
    session::ExecutionPhase,
    value::{
        carriers::ScopedValue,
        native::{NativeAllocation, NativeObject},
        view::ValueView,
    },
    value_check::data::async_scoped,
    value_check::scoped::CheckError,
};
use kagari_gc::root::Root;
use kagari_types::{declaration::native::NativeStorageLayout, ty::Ty};

impl<'call> ConversionContext<'call> {
    pub(in crate::native) fn for_cold_future(
        runtime: &'call mut Runtime,
        owner: &'call ExecutableModule,
        source: &'call Root<ColdObject>,
    ) -> NativeResult<Self> {
        runtime.validate_executable_module(owner)?;
        runtime.with_revision_origin(owner, RevisionSource::Cold(source), |_, _| Ok(()))?;
        let execution = runtime
            .resources
            .active_session()
            .ok_or_else(|| {
                RuntimeError::module_validation("cold producer requires an owned execution")
            })?
            .id;
        Self::prepare(
            runtime,
            owner,
            ConversionLimits::default(),
            Admission::Native {
                execution,
                origin: RevisionSource::Cold(source),
                metadata: Some(MetadataSource::Cold(source)),
            },
        )
    }

    /// Import a still-owned computation's immutable fields into one conversion
    /// prefix. The source root remains alive through every custom decoder.
    pub(in crate::native) fn cold_future_arguments(
        &mut self,
        source: &Root<ColdObject>,
    ) -> NativeResult<ArgumentValues<'call>> {
        let execution = self.execution_id()?;
        let Runtime {
            memory, resources, ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<ArgumentValues<'call>, MetadataError> {
                    let cold = slots.scope().get(source)?;
                    let count = slots.scope().read(cold, ColdObject::len);
                    let start = slots.len(execution, ValueBank::Local)?;
                    slots.reserve_locals(execution, count)?;
                    for index in 0..count {
                        if index.is_multiple_of(1024) {
                            resources.poll_closed()?;
                        }
                        let value = ColdObject::capture(slots.scope(), cold, index)?;
                        slots.push_local(execution, value)?;
                    }
                    Ok(ArgumentValues::new(execution, start, count))
                })())
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| match error {
                MetadataError::Memory(error) => self.gc_error(error),
                MetadataError::Execution(error) => {
                    self.runtime.resources.admit_execution_error(error)
                }
            })
    }

    pub(crate) fn allocate_future(
        &mut self,
        applied: &TypeArgument,
        recipe: ColdFuture,
        captures: &[ValueView<'_>],
    ) -> NativeResult<ValueView<'call>> {
        self.runtime
            .reject_static_initialization("cold Future construction")?;
        self.poll()?;
        if self.runtime.execution_options().phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation(
                "cold Future construction",
            ));
        }
        applied.validate(self.runtime)?;
        if recipe.owner().program_identity() != self.owner.program_identity() {
            return Err(RuntimeError::module_validation("cold producer owner"));
        }
        let Ty::NativeObject(nominal) = applied.ty() else {
            return Err(RuntimeError::module_validation("Future storage type"));
        };
        let storage = self
            .runtime
            .native_entries
            .storage
            .get_id(nominal.declaration)
            .cloned()
            .ok_or_else(|| RuntimeError::module_validation("Future storage registration"))?;
        if storage.descriptor().layout() != NativeStorageLayout::Future {
            return Err(RuntimeError::module_validation("Future storage role"));
        }
        let execution = self.execution_id()?;
        let origin = self.origin;
        let metadata = self.metadata;
        let owner = self.owner;
        let Runtime {
            memory,
            modules,
            resources,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<ValueView<'call>, MetadataError> {
                    let body = origin.resolve(modules, slots, owner)?;
                    let mut protected = match metadata.map(|source| source.resolve(modules, slots)).transpose()? {
                        Some(ProtectedMetadata::Cold(cold)) => ColdObject::metadata(slots.scope(), cold, resources)?,
                        Some(value) => vec![value],
                        None => Vec::new(),
                    };
                    protected.retain(|value| !matches!(value, ProtectedMetadata::Revision(value) if value.ptr_eq(body)));
                    let mut values = Vec::new();
                    values.try_reserve_exact(captures.len())?;
                    for (index, capture) in captures.iter().enumerate() {
                        if index.is_multiple_of(1024) { resources.poll_closed()?; }
                        values.push(capture.get(slots)?);
                    }
                    match async_scoped(slots.scope(), &values, resources) {
                        Ok(true) => {},
                        Ok(false) => return Err(RuntimeError::from(TrapReason::AsyncStateRetainsAnInvalidOrScopedValue).into()),
                        Err(CheckError::Storage(error)) => return Err(error.into()),
                        Err(CheckError::Execution(error)) => return Err(error.into()),
                    }
                    resources.poll_closed()?;
                    let cold = ColdObject::alloc(
                        slots.scope(),
                        recipe,
                        body,
                        &values,
                        &protected,
                        resources,
                    )?;
                    let payload = FuturePayload::alloc(slots.scope(), cold)?;
                    let header = NativeObject::alloc(
                        slots.scope(),
                        NativeAllocation {
                            origin: resources.candidate_origin(),
                            storage: storage.descriptor().clone(),
                            owner: owner.clone(),
                            applied: applied.clone(),
                        },
                        payload.erase(),
                        None,
                        &[],
                    )?;
                    let value = ScopedValue::managed(header);
                    let location = slots.push_local(execution, value)?;
                    Ok(ValueView::protected(value, Some(location))?)
                })())
            })
        });
        result
            .map_err(|error| self.gc_error(error))?
            .map_err(|error| match error {
                MetadataError::Memory(error) => self.gc_error(error),
                MetadataError::Execution(error) => {
                    self.runtime.resources.admit_execution_error(error)
                }
            })
    }
}
