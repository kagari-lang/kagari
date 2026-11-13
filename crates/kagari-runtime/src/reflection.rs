//! Reflection consumes only declared layout facts and actual protected data.
#[cfg(test)]
mod tests;
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind, reason::IndexError, trap::TrapReason},
    execution::roots::ExecutionRoots,
    frame::{arguments::FrameArguments, types::compatibility::TypeView},
    host::{HostBorrowTable, HostRegistry},
    module::StructLayoutRef,
    native::conversion::arguments::ArgumentValues,
    range::RangeValue,
    resource::ResourceState,
    value::{
        aggregates::{enumerations::EnumObject, structures::StructObject, tuples::TupleObject},
        arrays::ArrayObject,
        carriers::{RootedValue, ScopedValue},
        kind::ObjectKind,
        place::{PublishedResult, ResultPlace},
        properties::ValueProperties,
        strings::StringObject,
    },
    value_check::{
        self,
        data::candidate_access,
        scoped::{self, CheckError},
    },
};
use kagari_abi::representation::ValueType;
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject},
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{error}")]
pub struct ReflectionError {
    #[source]
    error: RuntimeError,
}

impl ReflectionError {
    pub fn new(reason: TrapReason) -> Self {
        Self {
            error: RuntimeError::from(reason),
        }
    }

    pub fn kind(&self) -> RuntimeErrorKind {
        self.error.kind()
    }

    pub fn into_runtime_error(self) -> RuntimeError {
        self.error
    }

    pub(crate) fn into_write_error(self) -> RuntimeError {
        if matches!(
            self.error.kind(),
            RuntimeErrorKind::ScriptTrap | RuntimeErrorKind::IndexOutOfBounds
        ) {
            self.error.into_reflective_write()
        } else {
            self.error
        }
    }
}

impl From<RuntimeError> for ReflectionError {
    fn from(error: RuntimeError) -> Self {
        Self { error }
    }
}

/// Runtime helpers and Host wrappers share one physical reflection policy.
#[derive(Clone, Copy)]
pub(crate) enum Operation<'name> {
    TypeOf,
    GetField(&'name str),
    SetField(&'name str),
    SetIndex,
}

impl Operation<'_> {
    fn arity(self) -> usize {
        match self {
            Self::TypeOf | Self::GetField(_) => 1,
            Self::SetField(_) => 2,
            Self::SetIndex => 3,
        }
    }

    fn error(self, error: RuntimeError) -> RuntimeError {
        match self {
            Self::TypeOf => error,
            Self::GetField(_)
                if matches!(
                    error.kind(),
                    RuntimeErrorKind::ScriptTrap | RuntimeErrorKind::IndexOutOfBounds
                ) =>
            {
                error.into_reflective_read()
            }
            Self::GetField(_) => error,
            Self::SetField(_) | Self::SetIndex => ReflectionError::from(error).into_write_error(),
        }
    }
}

/// Execute with the caller's original inputs and publish before the closed scope
/// ends. A callback cannot invalidate these values or repeat a completed write.
pub(crate) fn invoke<'result>(
    runtime: &mut Runtime,
    operation: Operation<'_>,
    arguments: ArgumentValues<'_>,
    destination: ResultPlace<'result>,
) -> Result<PublishedResult<'result>, RuntimeError> {
    runtime.resources.poll_execution()?;
    if arguments.len() != operation.arity() {
        return Err(RuntimeError::module_validation(
            "reflection helper argument count",
        ));
    }
    value_check::candidate_arguments(runtime, FrameArguments::pack(arguments))?;
    let Runtime {
        memory,
        host,
        host_borrows,
        resources,
        ..
    } = runtime;
    let result = memory.scope(resources, |cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            arguments.check_bounds(slots)?;
            let mut inputs = [ScopedValue::unit(); 3];
            for (index, input) in inputs.iter_mut().take(arguments.len()).enumerate() {
                *input = arguments.scoped(slots, index)?;
            }
            match apply(
                slots.scope(),
                operation,
                &inputs,
                host,
                host_borrows,
                resources,
            ) {
                Ok(value) => destination.publish_scoped(slots, value).map(Ok),
                Err(error) => Ok(Err(error)),
            }
        })
    });
    result
        .map_err(|error| runtime.managed_input_error(error))?
        .map_err(|error| {
            operation.error(
                runtime
                    .resources
                    .admit_execution_error(error.into_runtime(runtime)),
            )
        })
}

fn apply<'gc>(
    cx: &mut Scope<'gc>,
    operation: Operation<'_>,
    inputs: &[ScopedValue<'gc>; 3],
    host: &HostRegistry,
    borrows: &HostBorrowTable,
    resources: &ResourceState,
) -> Result<ScopedValue<'gc>, CheckError> {
    resources.poll_closed()?;
    match operation {
        Operation::TypeOf => {
            candidate_access(cx, inputs[0], resources)?;
            let name = type_name(cx, inputs[0])?;
            Ok(ScopedValue::managed(StringObject::alloc(cx, name)?))
        }
        Operation::GetField(name) => {
            let object = structure(
                cx,
                inputs[0],
                TrapReason::ReflectGetFieldExpectsStructValue,
                resources,
            )?;
            let (_, slot) = field(cx, object, name, resources)?;
            StructObject::get(cx, object, slot).map_err(Into::into)
        }
        Operation::SetField(name) => {
            let value = inputs[1];
            storable(
                cx,
                value,
                TrapReason::ReflectSetFieldExpectsDefaultStorableValue,
            )?;
            let object = structure(
                cx,
                inputs[0],
                TrapReason::ReflectSetFieldExpectsStructValue,
                resources,
            )?;
            let (layout, slot) = field(cx, object, name, resources)?;
            let contract = &layout.layout().fields[slot];
            let (ty, environment) = layout.field_type(slot).ok_or(GcError::Representation)?;
            if !contract.mutable
                || !scoped::matches(
                    cx,
                    value,
                    TypeView::new(ty, layout.module(), environment),
                    host,
                    borrows,
                    resources,
                )?
            {
                return Err(RuntimeError::from(
                    TrapReason::FieldIsReadOnlyOrValueHasTheWrongConcreteType,
                )
                .into());
            }
            candidate_access(cx, value, resources)?;
            resources.poll_closed()?;
            StructObject::set(cx, object, &layout, slot, value)?;
            Ok(inputs[0])
        }
        Operation::SetIndex => {
            let value = inputs[2];
            storable(
                cx,
                value,
                TrapReason::ReflectSetIndexExpectsDefaultStorableValue,
            )?;
            let index = index(inputs[1])?;
            candidate_access(cx, inputs[0], resources)?;
            candidate_access(cx, value, resources)?;
            match inputs[0].kind() {
                Some(ObjectKind::Array) => {
                    let array = object::<ArrayObject>(cx, inputs[0])?;
                    let valid = cx.read(array, |array| {
                        scoped::matches(
                            cx,
                            value,
                            array.contract().view(),
                            host,
                            borrows,
                            resources,
                        )
                    })?;
                    if !valid {
                        return Err(
                            RuntimeError::from(TrapReason::InvalidSequenceBufferOrPayload).into(),
                        );
                    }
                    let index = usize::try_from(index)
                        .ok()
                        .filter(|index| *index < cx.read(array, ArrayObject::len))
                        .ok_or_else(|| {
                            RuntimeError::from(IndexError::SequenceIndexIsOutOfBounds)
                        })?;
                    resources.poll_closed()?;
                    ArrayObject::set(cx, array, index, value)?;
                    Ok(inputs[0])
                }
                Some(ObjectKind::Tuple) => {
                    let tuple = object::<TupleObject>(cx, inputs[0])?;
                    let len = cx.read(tuple, TupleObject::len);
                    let slot = usize::try_from(index)
                        .ok()
                        .filter(|index| *index < len)
                        .ok_or_else(|| {
                            RuntimeError::from(TrapReason::InvalidReflectiveIndex { index })
                        })?;
                    let mut values = Vec::new();
                    values.try_reserve_exact(len)?;
                    for index in 0..len {
                        if index.is_multiple_of(1024) {
                            resources.poll_closed()?;
                        }
                        values.push(if index == slot {
                            value
                        } else {
                            TupleObject::get(cx, tuple, index)?
                        });
                    }
                    resources.poll_closed()?;
                    Ok(ScopedValue::managed(TupleObject::alloc(cx, &values)?))
                }
                _ => Err(
                    RuntimeError::from(TrapReason::ReflectSetIndexExpectsArrayOrTupleValue).into(),
                ),
            }
        }
    }
}

fn object<'gc, T: GcObject>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
) -> Result<Gc<'gc, T>, GcError> {
    cx.downcast(value.object().ok_or(GcError::Representation)?)
}

fn structure<'gc>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
    reason: TrapReason,
    resources: &ResourceState,
) -> Result<Gc<'gc, StructObject>, CheckError> {
    if value.kind() != Some(ObjectKind::Struct) {
        return Err(RuntimeError::from(reason).into());
    }
    candidate_access(cx, value, resources)?;
    object(cx, value).map_err(Into::into)
}

fn field<'gc>(
    cx: &Scope<'gc>,
    object: Gc<'gc, StructObject>,
    name: &str,
    resources: &ResourceState,
) -> Result<(StructLayoutRef, usize), CheckError> {
    let layout = cx.read(object, |object| object.layout().clone());
    let mut slot = None;
    for (index, field) in layout.layout().fields.iter().enumerate() {
        if index.is_multiple_of(1024) {
            resources.poll_closed()?;
        }
        if field.name == name {
            slot = Some(index);
            break;
        }
    }
    let slot = slot.ok_or_else(|| {
        RuntimeError::from(TrapReason::MissingField {
            name: name.to_owned(),
        })
    })?;
    Ok((layout, slot))
}

fn storable<'gc>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
    reason: TrapReason,
) -> Result<(), CheckError> {
    if !ValueProperties::of(cx, value)?.storable {
        return Err(RuntimeError::from(reason).into());
    }
    Ok(())
}

fn index(value: ScopedValue<'_>) -> Result<u64, RuntimeError> {
    match value.scalar_payload() {
        Some((ValueType::I32 | ValueType::I64, bits)) if bits as i64 >= 0 => Ok(bits),
        Some((ValueType::U64, bits)) => Ok(bits),
        _ => Err(TrapReason::ReflectSetIndexExpectsNonNegativeIntegerIndex.into()),
    }
}

fn type_name<'gc>(cx: &Scope<'gc>, value: ScopedValue<'gc>) -> Result<String, GcError> {
    let name = match value.kind() {
        Some(ObjectKind::Range) => {
            cx.read(object::<RangeValue>(cx, value)?, |range| range.kind.name())
        }
        Some(ObjectKind::String) => "String",
        Some(ObjectKind::Tuple) => "tuple",
        Some(ObjectKind::Array) => "array",
        Some(ObjectKind::Native) => "gc_handle",
        Some(ObjectKind::Interface) => "interface",
        Some(ObjectKind::Closure) => "closure",
        Some(ObjectKind::Cell) => "cell",
        Some(ObjectKind::HostRoot) => "host_root",
        Some(ObjectKind::HostPath) => "host_path_view",
        Some(ObjectKind::Ephemeral) => "ephemeral",
        Some(ObjectKind::Enum) => {
            return cx.read(object::<EnumObject>(cx, value)?, |value| {
                let layout = value.layout();
                layout
                    .module()
                    .definition_name(layout.layout().declaration)
                    .map(str::to_owned)
                    .ok_or(GcError::Representation)
            });
        }
        Some(ObjectKind::Struct) => {
            return cx.read(object::<StructObject>(cx, value)?, |value| {
                let layout = value.layout();
                layout
                    .module()
                    .definition_name(layout.layout().declaration)
                    .map(str::to_owned)
                    .ok_or(GcError::Representation)
            });
        }
        None => match value.scalar_payload().map(|(kind, _)| kind) {
            Some(ValueType::Unit) => "()",
            Some(ValueType::Bool) => "bool",
            Some(ValueType::Char) => "char",
            Some(ValueType::I32) => "i32",
            Some(ValueType::I64) => "i64",
            Some(ValueType::U64) => "u64",
            Some(ValueType::F32) => "f32",
            Some(ValueType::F64) => "f64",
            _ => return Err(GcError::Representation),
        },
    };
    Ok(name.to_owned())
}

impl Runtime {
    fn reflect_rooted(
        &mut self,
        operation: Operation<'_>,
        values: &[&RootedValue],
    ) -> Result<RootedValue, RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.poll_execution()?;
        for value in values {
            value_check::candidate_root(self, value)?;
        }
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = self;
        let result = memory.scope(resources, |cx, _| {
            let mut inputs = [ScopedValue::unit(); 3];
            for (input, value) in inputs.iter_mut().zip(values) {
                *input = value.get(cx)?;
            }
            match apply(cx, operation, &inputs, host, host_borrows, resources) {
                Ok(value) => {
                    // A successful mutable setter returns its exact receiver.
                    // Reuse the original genuine lease, without another registration.
                    if matches!(operation, Operation::SetField(_))
                        || matches!(operation, Operation::SetIndex)
                            && inputs[0].kind() == Some(ObjectKind::Array)
                    {
                        Ok(Ok(values[0].clone()))
                    } else {
                        value.retain(cx).map(Ok)
                    }
                }
                Err(error) => Ok(Err(error)),
            }
        });
        result
            .map_err(|error| self.managed_input_error(error))?
            .map_err(|error| {
                operation.error(
                    self.resources
                        .admit_execution_error(error.into_runtime(self)),
                )
            })
    }

    pub fn reflect_type_of(&mut self, value: &RootedValue) -> Result<RootedValue, RuntimeError> {
        self.reflect_rooted(Operation::TypeOf, &[value])
    }

    pub fn reflect_get_field(
        &mut self,
        value: &RootedValue,
        name: &str,
    ) -> Result<RootedValue, RuntimeError> {
        self.reflect_rooted(Operation::GetField(name), &[value])
    }

    pub fn reflect_set_field(
        &mut self,
        value: &RootedValue,
        name: &str,
        next: &RootedValue,
    ) -> Result<RootedValue, RuntimeError> {
        self.reflect_rooted(Operation::SetField(name), &[value, next])
    }

    pub fn reflect_set_index(
        &mut self,
        value: &RootedValue,
        index: &RootedValue,
        next: &RootedValue,
    ) -> Result<RootedValue, RuntimeError> {
        self.reflect_rooted(Operation::SetIndex, &[value, index, next])
    }
}
