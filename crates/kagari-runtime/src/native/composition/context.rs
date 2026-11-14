//! Policy reads and plain aggregate construction use genuine output roots.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, compatibility::TypeView},
    native::{
        binding::NativeResult,
        composition::{PairContext, invalid},
        types::VariantRef,
    },
    value::{
        EnumTag,
        aggregates::{enumerations::EnumObject, tuples::TupleObject},
        carriers::{RootedValue, ScopedValue},
        kind::ObjectKind,
    },
    value_check::scoped::{self, CheckError},
};
use kagari_gc::{access::Scope, error::GcError, object::Gc};
use kagari_types::ty::Ty;

impl PairContext<'_> {
    pub fn input_type(&self, index: usize) -> NativeResult<&TypeArgument> {
        self.contract.inputs.get(index).ok_or_else(invalid)
    }

    pub fn output_type(&self) -> &TypeArgument {
        &self.contract.output
    }

    pub fn parameter(&self, ty: &TypeArgument, index: usize) -> NativeResult<TypeArgument> {
        ty.parameter(self.runtime, &self.contract.owner, index)
    }

    pub fn compatible(&self, left: &TypeArgument, right: &TypeArgument) -> bool {
        left.view(&self.contract.owner)
            .compatible(right.view(&self.contract.owner))
    }

    pub fn check_variant(&self, ty: &TypeArgument, variant: &VariantRef) -> NativeResult<()> {
        self.runtime
            .checked_enum_variant(&self.contract.owner, ty, variant)
            .map(|_| ())
    }

    /// Clone the existing output lease, without a new root registration or read.
    pub fn output(&self, index: usize) -> NativeResult<Option<RootedValue>> {
        self.outputs.get(index).cloned().ok_or_else(invalid)
    }

    pub fn is_variant(&mut self, index: usize, variant: &VariantRef) -> NativeResult<bool> {
        let expected = self.runtime.checked_enum_variant(
            &self.contract.owner,
            self.input_type(index)?,
            variant,
        )?;
        let value = self.output(index)?.ok_or_else(invalid)?;
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, _| {
                let object = enumeration(cx, &value)?;
                Ok(cx.read(object, |object| object.layout().matches_layout(&expected)))
            })
            .map_err(|error| storage_error(self.runtime, error))
    }

    /// A managed child gains exactly one escaping lease; scalar fields stay inline.
    pub fn field(
        &mut self,
        index: usize,
        variant: &VariantRef,
        field: usize,
    ) -> NativeResult<RootedValue> {
        let expected = self.runtime.checked_enum_variant(
            &self.contract.owner,
            self.input_type(index)?,
            variant,
        )?;
        let value = self.output(index)?.ok_or_else(invalid)?;
        self.runtime
            .memory
            .scope(&self.runtime.resources, |cx, _| {
                let object = enumeration(cx, &value)?;
                if !cx.read(object, |object| object.layout().matches_layout(&expected)) {
                    return Err(GcError::Representation);
                }
                EnumObject::get(cx, object, field)?.retain(cx)
            })
            .map_err(|error| storage_error(self.runtime, error))
    }

    pub fn tuple(
        &mut self,
        ty: &TypeArgument,
        values: Vec<RootedValue>,
    ) -> NativeResult<RootedValue> {
        self.runtime.resources.poll_execution()?;
        let Ty::Tuple(elements) = ty.ty() else {
            return Err(invalid());
        };
        if elements.len() != values.len() {
            return Err(invalid());
        }
        let mut types = Vec::new();
        types
            .try_reserve_exact(elements.len())
            .map_err(|_| RuntimeError::resource_limit("composition tuple types"))?;
        for index in 0..elements.len() {
            if index.is_multiple_of(1024) {
                self.runtime.resources.poll_execution()?;
            }
            types.push(self.parameter(ty, index)?);
        }
        let owner = &self.contract.owner;
        let Runtime {
            memory,
            host,
            host_borrows,
            resources,
            ..
        } = &mut *self.runtime;
        let result = memory.scope(resources, |cx, _| {
            Ok((|| -> Result<_, CheckError> {
                let mut fields = Vec::new();
                fields.try_reserve_exact(values.len())?;
                for (index, (root, ty)) in values.iter().zip(&types).enumerate() {
                    if index.is_multiple_of(1024) {
                        resources.poll_closed()?;
                    }
                    let value = root.get(cx)?;
                    if !scoped::matches(cx, value, ty.view(owner), host, host_borrows, resources)? {
                        return Err(invalid().into());
                    }
                    fields.push(value);
                }
                let object = TupleObject::alloc(cx, &fields)?;
                Ok(ScopedValue::managed(object).retain(cx)?)
            })())
        });
        finish(self.runtime, result)
    }

    pub fn allocate_enum(
        &mut self,
        ty: &TypeArgument,
        variant: &VariantRef,
        fields: Vec<RootedValue>,
    ) -> NativeResult<RootedValue> {
        let layout = self
            .runtime
            .checked_enum_variant(&self.contract.owner, ty, variant)?;
        self.runtime.alloc_enum(EnumTag::Declared(layout), &fields)
    }

    /// Rebind one declared variant while preserving the optional diagnostic Arc,
    /// including absence. Matching payload scopes precede physical allocation.
    pub fn forward_variant(
        &mut self,
        index: usize,
        target: &TypeArgument,
        variant: &VariantRef,
    ) -> NativeResult<RootedValue> {
        self.runtime.resources.poll_execution()?;
        let source = self.runtime.checked_enum_variant(
            &self.contract.owner,
            self.input_type(index)?,
            variant,
        )?;
        let destination =
            self.runtime
                .checked_enum_variant(&self.contract.owner, target, variant)?;
        let original = self.output(index)?.ok_or_else(invalid)?;
        if source.variant().payload.len() != destination.variant().payload.len() {
            return Err(invalid());
        }
        for index in 0..source.variant().payload.len() {
            if index.is_multiple_of(1024) {
                self.runtime.resources.poll_execution()?;
            }
            let (source_ty, source_environment) = source.payload_type(index).ok_or_else(invalid)?;
            let (target_ty, target_environment) =
                destination.payload_type(index).ok_or_else(invalid)?;
            if !TypeView::new(source_ty, source.module(), source_environment).compatible(
                TypeView::new(target_ty, destination.module(), target_environment),
            ) {
                return Err(invalid());
            }
        }
        let resources = &self.runtime.resources;
        let result = self.runtime.memory.scope(&self.runtime.resources, |cx, _| {
            Ok((|| -> Result<_, CheckError> {
                let object = enumeration(cx, &original)?;
                if !cx.read(object, |object| object.layout().matches_layout(&source)) {
                    return Err(invalid().into());
                }
                let (count, origin) =
                    cx.read(object, |object| (object.len(), object.origin().cloned()));
                let mut fields = Vec::new();
                fields.try_reserve_exact(count)?;
                for index in 0..count {
                    if index.is_multiple_of(1024) {
                        resources.poll_closed()?;
                    }
                    fields.push(EnumObject::get(cx, object, index)?);
                }
                let object = EnumObject::alloc(cx, destination, &fields, origin)?;
                Ok(ScopedValue::managed(object).retain(cx)?)
            })())
        });
        finish(self.runtime, result)
    }
}

fn enumeration<'gc>(cx: &Scope<'gc>, value: &RootedValue) -> Result<Gc<'gc, EnumObject>, GcError> {
    let value = value.get(cx)?;
    if value.kind() != Some(ObjectKind::Enum) {
        return Err(GcError::Representation);
    }
    cx.downcast(value.object().ok_or(GcError::Representation)?)
}

fn finish<T>(runtime: &Runtime, result: Result<Result<T, CheckError>, GcError>) -> NativeResult<T> {
    result
        .map_err(|error| storage_error(runtime, error))?
        .map_err(|error| {
            runtime
                .resources
                .admit_execution_error(error.into_runtime(runtime))
        })
}

fn storage_error(runtime: &Runtime, error: GcError) -> RuntimeError {
    match error {
        GcError::ForeignHeap | GcError::InvalidRoot | GcError::Index | GcError::Representation => {
            invalid()
        }
        error => runtime.managed_memory_error(error),
    }
}
