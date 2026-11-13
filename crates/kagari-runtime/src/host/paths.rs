//! Actual path objects supply callback snapshots and explicit Host retention.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{ExecutionRoots, SessionId, Slots},
    host::{
        DynamicPathArgument, DynamicPathArguments, HostCallContext, HostPathContext,
        HostPathDescriptor, HostPathDescriptorId, HostPathOperation, HostRegistry, HostRootHandle,
        path_scope_error,
    },
    resource::ResourceState,
    value::{
        carriers::{RootedValue, ScopedValue},
        host_values::{
            HostRootObject,
            paths::{HostPathObject, PathFacts},
        },
        kind::ObjectKind,
        properties::ValueProperties,
        strings::StringObject,
        view::ValueView,
    },
    value_check::scoped::CheckError,
};
use kagari_gc::{access::Scope, error::GcError};
use kagari_types::declaration::host::PathAccess;

impl Runtime {
    /// Wrap a registered external root in one genuine Host-owned value. The heap
    /// stores only its checked capability; Rust object state remains outside it.
    pub fn host_root_value(&mut self, root: HostRootHandle) -> Result<RootedValue, RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.ensure_execution_allowed()?;
        self.reject_candidate_external_access()?;
        if !self.host.matches_root(root) {
            return Err(invalid("host root is not registered"));
        }
        self.memory
            .scope(&self.resources, |cx, _| {
                ScopedValue::managed(HostRootObject::alloc(cx, root)?).retain(cx)
            })
            .map_err(|error| self.managed_memory_error(error))
    }

    /// Build a path from a genuine root/path value and genuine dynamic arguments.
    /// The returned root protects the actual provider, its base and argument edges.
    pub fn make_host_path_view(
        &mut self,
        root: &RootedValue,
        descriptor: HostPathDescriptorId,
        arguments: &[RootedValue],
    ) -> Result<RootedValue, RuntimeError> {
        self.resources.ensure_execution_allowed()?;
        self.reject_candidate_external_access()?;
        let mut call = HostCallContext::new(self, &[])?;
        let root = call.import(root).map_err(path_scope_error)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(arguments.len())
            .map_err(|_| RuntimeError::resource_limit("path arguments"))?;
        for value in arguments {
            values.push(call.import(value).map_err(path_scope_error)?);
        }
        let value = call.make_path_view(&root, descriptor, &values)?;
        call.retain(&value)
    }

    pub(in crate::host) fn with_path_values<R>(
        &mut self,
        access: impl for<'gc> FnOnce(
            &mut Slots<'_, 'gc>,
            &HostRegistry,
            &ResourceState,
        ) -> Result<R, CheckError>,
    ) -> Result<R, RuntimeError> {
        let Self {
            memory,
            host,
            resources,
            ..
        } = self;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| {
                    resources.poll_closed()?;
                    access(slots, host, resources)
                })())
            })
        });
        result
            .map_err(|error| self.managed_memory_error(error))?
            .map_err(|error| {
                self.resources
                    .admit_execution_error(error.into_runtime(self))
            })
    }

    pub(in crate::host) fn prepare_path_context<'call>(
        &mut self,
        root: &ValueView<'call>,
        descriptor: HostPathDescriptorId,
        arguments: &[ValueView<'call>],
        operation: HostPathOperation,
    ) -> Result<HostPathContext<ValueView<'call>>, RuntimeError> {
        let descriptor = self
            .host
            .path_descriptor(descriptor)
            .cloned()
            .ok_or_else(|| invalid("path descriptor is not registered"))?;
        if operation.writes() && descriptor.access != PathAccess::ReadWrite {
            return Err(invalid("path descriptor is not writable"));
        }
        if descriptor.dynamic_parameters.len() != arguments.len() {
            return Err(invalid(
                "path dynamic argument count differs from descriptor",
            ));
        }
        self.with_path_values(|slots, host, resources| {
            for (index, value) in arguments.iter().enumerate() {
                if index.is_multiple_of(1024) {
                    resources.poll_closed()?;
                }
                let value = value.get(slots)?;
                require_storable(slots.scope(), value)?;
            }
            let value = root.get(slots)?;
            let (handle, base_access) = root_facts(
                slots.scope(),
                host,
                resources,
                value,
                &descriptor,
                operation,
            )?;
            let mut dynamic = Vec::new();
            dynamic.try_reserve_exact(arguments.len())?;
            dynamic.extend(
                descriptor
                    .dynamic_parameters
                    .iter()
                    .zip(arguments)
                    .map(|(parameter, value)| DynamicPathArgument::new(parameter.ty, *value)),
            );
            Ok(HostPathContext {
                root: handle,
                base_view: base_access.map(|_| *root),
                access: if base_access == Some(PathAccess::ReadOnly) {
                    PathAccess::ReadOnly
                } else {
                    descriptor.access
                },
                descriptor: descriptor.clone(),
                dynamic_args: DynamicPathArguments::new(dynamic),
            })
        })
    }
}

impl<'call> HostCallContext<'call> {
    /// Transfer owned text into this invocation's source. Returning
    /// it from a read callback does not require a temporary Host root.
    pub fn allocate_string(&mut self, text: String) -> Result<ValueView<'call>, RuntimeError> {
        let execution = self.scope.execution_id();
        self.runtime_mut().with_path_values(|slots, _, _| {
            let value = ScopedValue::managed(StringObject::alloc(slots.scope(), text)?);
            protect(slots, execution, value).map_err(CheckError::from)
        })
    }

    /// Inspect one actual path component. A base remains an opaque protected
    /// value and may be inspected in turn; no recursive descriptor copy is made.
    pub fn inspect_path(
        &mut self,
        value: &ValueView<'_>,
    ) -> Result<HostPathContext<ValueView<'call>>, RuntimeError> {
        let execution = self.scope.execution_id();
        self.runtime_mut()
            .with_path_values(|slots, host, resources| {
                let value = value.get(slots)?;
                if value.kind() != Some(ObjectKind::HostPath) {
                    return Err(invalid("expected host path view").into());
                }
                let object = slots
                    .scope()
                    .downcast::<HostPathObject>(value.object().ok_or(GcError::Representation)?)?;
                host.validate_path_scoped(slots.scope(), object, resources)??;
                let facts = slots.scope().read(object, HostPathObject::facts);
                let descriptor = host
                    .path_descriptor(facts.descriptor)
                    .cloned()
                    .ok_or_else(|| invalid("path descriptor is not registered"))?;
                let count = slots.scope().read(object, HostPathObject::len);
                let mut arguments = Vec::new();
                arguments.try_reserve_exact(count)?;
                for index in 0..count {
                    if index.is_multiple_of(1024) {
                        resources.poll_closed()?;
                    }
                    let ty = slots
                        .scope()
                        .read(object, |object| object.argument_type(index))
                        .ok_or(GcError::Index)?;
                    let value = HostPathObject::argument(slots.scope(), object, index)?;
                    arguments.push(DynamicPathArgument::new(
                        ty,
                        protect(slots, execution, value)?,
                    ));
                }
                let base_view = HostPathObject::base(slots.scope(), object)
                    .map(|base| protect(slots, execution, ScopedValue::managed(base)))
                    .transpose()?;
                Ok(HostPathContext {
                    root: facts.root,
                    base_view,
                    access: facts.access,
                    descriptor,
                    dynamic_args: DynamicPathArguments::new(arguments),
                })
            })
    }

    pub(crate) fn make_path_view(
        &mut self,
        root: &ValueView<'call>,
        descriptor: HostPathDescriptorId,
        arguments: &[ValueView<'call>],
    ) -> Result<ValueView<'call>, RuntimeError> {
        let context = self.runtime_mut().prepare_path_context(
            root,
            descriptor,
            arguments,
            HostPathOperation::MakeView,
        )?;
        let execution = self.scope.execution_id();
        self.runtime_mut().with_path_values(|slots, _, resources| {
            let base = context
                .base_view
                .map(|value| {
                    let value = value.get(slots)?;
                    slots
                        .scope()
                        .downcast::<HostPathObject>(value.object().ok_or(GcError::Representation)?)
                })
                .transpose()?;
            let mut input = Vec::new();
            input.try_reserve_exact(arguments.len())?;
            for (index, argument) in context.dynamic_args.as_slice().iter().enumerate() {
                if index.is_multiple_of(1024) {
                    resources.poll_closed()?;
                }
                input.push((argument.ty, argument.value.get(slots)?));
            }
            let object = HostPathObject::alloc(
                slots.scope(),
                PathFacts {
                    root: context.root,
                    descriptor,
                    result: context.descriptor.result_type,
                    access: context.access,
                    epoch: context.descriptor.schema_epoch,
                },
                base,
                &input,
            )?;
            protect(slots, execution, ScopedValue::managed(object)).map_err(CheckError::from)
        })
    }

    pub(in crate::host) fn require_storable(
        &mut self,
        value: &ValueView<'_>,
    ) -> Result<(), RuntimeError> {
        self.runtime_mut().with_path_values(|slots, _, _| {
            let value = value.get(slots)?;
            require_storable(slots.scope(), value)
        })
    }
}

fn require_storable<'gc>(cx: &Scope<'gc>, value: ScopedValue<'gc>) -> Result<(), CheckError> {
    if !ValueProperties::of(cx, value)?.storable {
        return Err(invalid("path value must be a default heap payload").into());
    }
    Ok(())
}

fn root_facts<'gc>(
    cx: &Scope<'gc>,
    host: &HostRegistry,
    resources: &ResourceState,
    value: ScopedValue<'gc>,
    descriptor: &HostPathDescriptor,
    operation: HostPathOperation,
) -> Result<(HostRootHandle, Option<PathAccess>), CheckError> {
    let object = value
        .object()
        .ok_or_else(|| invalid("path operation expects host root or path view"))?;
    let (root, input, epoch, path) = match value.kind() {
        Some(ObjectKind::HostRoot) => {
            let object = cx.downcast::<HostRootObject>(object)?;
            let root = cx.read(object, HostRootObject::handle);
            (root, root.type_id(), root.schema_epoch(), None)
        }
        Some(ObjectKind::HostPath) => {
            let object = cx.downcast::<HostPathObject>(object)?;
            host.validate_path_scoped(cx, object, resources)??;
            let facts = cx.read(object, HostPathObject::facts);
            if operation.writes() && facts.access != PathAccess::ReadWrite {
                return Err(invalid("host path view is not writable").into());
            }
            (facts.root, facts.result, facts.epoch, Some(facts.access))
        }
        _ => return Err(invalid("path operation expects host root or path view").into()),
    };
    if !host.matches_root(root) {
        return Err(invalid("host root is not registered").into());
    }
    if descriptor.root_type != input {
        return Err(invalid("path descriptor root type differs from receiver").into());
    }
    if descriptor.schema_epoch != epoch {
        return Err(invalid("path descriptor schema epoch differs from receiver").into());
    }
    Ok((root, path))
}

fn protect<'gc, 'call>(
    slots: &mut Slots<'_, 'gc>,
    execution: SessionId,
    value: ScopedValue<'gc>,
) -> Result<ValueView<'call>, GcError> {
    let location = value
        .object()
        .map(|_| slots.push_local(execution, value))
        .transpose()?;
    ValueView::protected(value, location)
}

fn invalid(message: &'static str) -> RuntimeError {
    RuntimeError::typed_path_validation(message)
}
