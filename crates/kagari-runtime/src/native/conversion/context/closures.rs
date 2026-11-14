//! Closure creation publishes captured values and exact body/environment edges.
use crate::{
    Runtime,
    closure::{ClosureMode, ClosureRecord, calls::SelectedClosure},
    error::RuntimeError,
    execution::roots::ExecutionRoots,
    execution_metadata::managed::{
        ProtectedGraph, ProtectedMetadata,
        sources::{MetadataError, MetadataSource},
    },
    frame::types::{TypeEnvironment, arguments::ScopedSignature},
    native::{
        binding::NativeResult, context::LinkedCallable, conversion::context::ConversionContext,
    },
    value::{
        carriers::ScopedValue, cells::CellObject, closures::ClosureObject, kind::ObjectKind,
        view::ValueView,
    },
    value_check::scoped::{self, CheckError},
};
use kagari_bytecode::module::CallableTarget;
use kagari_contract::{ids::FunctionRef, representation::semantic_representation};
use kagari_gc::error::GcError;
use std::{ptr, slice, sync::Arc};

impl<'call> ConversionContext<'call> {
    pub(crate) fn allocate_script_closure(
        &mut self,
        function: FunctionRef,
        captures: &[ValueView<'_>],
        environment: Option<TypeEnvironment>,
        mode: ClosureMode,
    ) -> NativeResult<ValueView<'call>> {
        let record = ClosureRecord::script(
            self.runtime,
            self.owner,
            function,
            captures.len(),
            environment,
            mode,
        )?;
        self.publish_closure(Arc::new(record), captures)
    }

    pub(crate) fn allocate_native_closure(
        &mut self,
        target: &LinkedCallable,
        signature: Arc<ScopedSignature>,
        mode: ClosureMode,
    ) -> NativeResult<ValueView<'call>> {
        let owner = target.owner(self.owner)?;
        if owner.program_identity() != self.owner.program_identity()
            || owner.slot() != self.owner.slot()
            || target.primitive.is_some()
        {
            return Err(RuntimeError::module_validation(
                "native closure conversion owner",
            ));
        }
        let CallableTarget::Native(import) = target.target else {
            return Err(RuntimeError::module_validation(
                "native closure requires a native entry",
            ));
        };
        let record = ClosureRecord::native(
            self.runtime,
            owner,
            import,
            target.environment.clone(),
            signature,
            mode,
        )?;
        self.publish_closure(Arc::new(record), &[])
    }

    pub(crate) fn publish_closure(
        &mut self,
        record: Arc<ClosureRecord>,
        captures: &[ValueView<'_>],
    ) -> NativeResult<ValueView<'call>> {
        self.poll()?;
        if record.implementation.program_identity() != self.owner.program_identity()
            || record.implementation.slot() != self.owner.slot()
        {
            return Err(RuntimeError::module_validation("closure publication owner"));
        }
        let execution = self.execution_id()?;
        let revision = MetadataSource::Revision {
            owner: self.owner,
            source: self.origin,
        };
        let pair = self.metadata.map(|metadata| [revision, metadata]);
        let sources = pair
            .as_ref()
            .map_or(slice::from_ref(&revision), |pair| pair.as_slice());
        if let Some(environment) = &record.environment {
            self.runtime.validate_environment_in(sources, environment)?;
        }
        let origin = self.origin;
        let owner = self.owner;
        let metadata = self.metadata;
        let Runtime {
            memory,
            modules,
            resources,
            host,
            host_borrows,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<ValueView<'call>, MetadataError> {
                    // Admit originals before allocation/publication can reuse any
                    // source tail. The body is never recovered from code identity.
                    let body = origin.resolve(modules, slots, owner)?;
                    let revision = ProtectedMetadata::Revision(body);
                    let pair = metadata
                        .map(|metadata| {
                            metadata
                                .resolve(modules, slots)
                                .map(|metadata| [revision, metadata])
                        })
                        .transpose()?;
                    let protected = pair
                        .as_ref()
                        .map_or(slice::from_ref(&revision), |pair| pair.as_slice());
                    let graph = ProtectedGraph::new(slots.scope(), protected, resources)?;
                    let environment = record
                        .environment
                        .as_ref()
                        .map(|facts| {
                            graph
                                .environment(slots.scope(), facts, resources)?
                                .ok_or(MetadataError::Memory(GcError::Representation))
                        })
                        .transpose()?;
                    let mut values = Vec::new();
                    values.try_reserve_exact(captures.len())?;
                    for (index, value) in captures.iter().enumerate() {
                        if index.is_multiple_of(1024) {
                            resources.poll_closed()?;
                        }
                        let value = value.get(slots)?;
                        if let Some(expected) =
                            record.capture_types.get(index).and_then(Option::as_ref)
                        {
                            let content = if value.kind() == Some(ObjectKind::Cell) {
                                let cell = slots.scope().downcast::<CellObject>(
                                    value.object().ok_or(GcError::Representation)?,
                                )?;
                                CellObject::get(
                                    slots.scope(),
                                    cell,
                                    semantic_representation(expected.ty()),
                                )?
                            } else {
                                value
                            };
                            if !scoped::matches(
                                slots.scope(),
                                content,
                                expected.view(owner),
                                host,
                                host_borrows,
                                resources,
                            )
                            .map_err(|error| match error {
                                CheckError::Storage(error) => MetadataError::Memory(error),
                                CheckError::Execution(error) => MetadataError::Execution(error),
                            })? {
                                return Err(RuntimeError::module_validation(
                                    "closure semantic capture mismatch",
                                )
                                .into());
                            }
                        }
                        values.push(value);
                    }
                    resources.poll_closed()?;
                    let object = ClosureObject::alloc(
                        slots.scope(),
                        record,
                        body,
                        environment,
                        &values,
                        resources.candidate_origin(),
                    )?;
                    let value = ScopedValue::managed(object);
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

impl<'call> ConversionContext<'call> {
    /// Freeze a selected named relationship only for a genuine retained computation.
    /// Exact objects reuse their identity; ordinary callback invocation does not
    /// allocate this retained object or register a Host root.
    pub(crate) fn freeze_closure(
        &mut self,
        selected: &SelectedClosure<'_>,
    ) -> NativeResult<ValueView<'call>> {
        self.poll()?;
        let execution = self.execution_id()?;
        let original = selected.value();
        if selected.record().call_route().is_none() {
            return self.with_values(|slots| {
                let value = original.get(slots)?;
                let location = slots.push_local(execution, value)?;
                ValueView::protected(value, Some(location))
            });
        }
        let record = selected.retained_record(&self.runtime.resources)?;
        let environment = selected.environment();
        let Runtime {
            memory,
            resources,
            modules,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<ValueView<'call>, MetadataError> {
                    let original = original.get(slots)?;
                    let object = slots.scope().downcast::<ClosureObject>(
                        original.object().ok_or(GcError::Representation)?,
                    )?;
                    if !slots.scope().read(object, |object| {
                        ptr::eq(object.record().as_ref(), selected.record())
                    }) {
                        return Err(GcError::Representation.into());
                    }
                    let body = selected
                        .origin()
                        .resolve(modules, slots, selected.owner())?;
                    let environment = environment
                        .as_ref()
                        .map(|(facts, source)| -> Result<_, MetadataError> {
                            let root = source.resolve(modules, slots)?;
                            let graph = ProtectedGraph::new(
                                slots.scope(),
                                slice::from_ref(&root),
                                resources,
                            )?;
                            graph
                                .environment(slots.scope(), facts, resources)?
                                .ok_or_else(|| GcError::Representation.into())
                        })
                        .transpose()?;
                    let mut captures = Vec::new();
                    captures.try_reserve_exact(record.capture_types.len())?;
                    for index in 0..record.capture_types.len() {
                        if index.is_multiple_of(1024) {
                            resources.poll_closed()?;
                        }
                        captures.push(ClosureObject::capture(slots.scope(), object, index)?);
                    }
                    let object = ClosureObject::alloc(
                        slots.scope(),
                        record,
                        body,
                        environment,
                        &captures,
                        resources.candidate_origin(),
                    )?;
                    let value = ScopedValue::managed(object);
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
