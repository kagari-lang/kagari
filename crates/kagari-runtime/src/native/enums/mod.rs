//! Prepared ordinary enum constructors with scoped, retained payload conversion.
mod binding;
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::{EnumVariantRef, LoadedModule, retention::ProgramLease},
    native::{
        binding::NativeResult,
        conversion::{
            arguments::{IntoKagariArguments, encode_arguments},
            context::ConversionContext,
        },
        typed::NativeContext,
        value_handle::ScriptValue,
    },
    value::{EnumTag, Value},
};
use kagari_common::identity::{DefinitionKind, DefinitionPath};
use std::{marker::PhantomData, sync::Arc};

#[derive(Debug, Clone)]
pub struct EnumType(Arc<TypeRecord>);

#[derive(Debug)]
struct TypeRecord {
    owner: LoadedModule,
    argument: TypeArgument,
    _program: ProgramLease,
}

#[derive(Debug, Clone)]
pub struct EnumMember {
    enum_type: EnumType,
    layout: EnumVariantRef,
}

/// The Rust tuple contains one element per declared payload field. An empty
/// variant uses `()`; a single unit payload uses `((),)`.
#[derive(Debug)]
pub struct EnumVariant<A> {
    record: Arc<VariantRecord>,
    mapping: PhantomData<fn(A)>,
}

#[derive(Debug)]
struct VariantRecord {
    member: EnumMember,
    payload: Vec<TypeArgument>,
}

impl<A> Clone for EnumVariant<A> {
    fn clone(&self) -> Self {
        Self {
            record: self.record.clone(),
            mapping: PhantomData,
        }
    }
}

impl EnumType {
    pub fn type_argument(&self) -> &TypeArgument {
        &self.0.argument
    }

    pub fn owner(&self) -> &LoadedModule {
        &self.0.owner
    }

    pub fn variant(&self, runtime: &Runtime, name: &str) -> NativeResult<EnumMember> {
        runtime.validate_loaded_module(self.owner())?;
        let layout = runtime.declared_enum_variant(self.owner(), self.type_argument(), name)?;
        Ok(EnumMember {
            enum_type: self.clone(),
            layout,
        })
    }

    /// Registration handles supply their declaration identity through `id()`.
    pub fn variant_declaration(
        &self,
        runtime: &Runtime,
        declaration: &DefinitionPath,
    ) -> NativeResult<EnumMember> {
        let segment = declaration
            .path
            .last()
            .filter(|segment| segment.kind == DefinitionKind::Variant)
            .ok_or_else(|| RuntimeError::module_validation("enum variant declaration"))?;
        let member = self.variant(runtime, &segment.name)?;
        if member
            .layout
            .module()
            .definition(member.layout.variant().declaration)?
            .to_path()
            != *declaration
        {
            return Err(RuntimeError::module_validation(
                "variant belongs to another enum",
            ));
        }
        Ok(member)
    }
}

impl<A: IntoKagariArguments> EnumVariant<A> {
    /// Complete conversion and validation before publishing the enum object.
    /// The result retains its exact nominal type and every reachable payload.
    pub fn create(&self, cx: &mut NativeContext<'_>, payload: A) -> NativeResult<ScriptValue> {
        let record = &self.record;
        let ty = &record.member.enum_type;
        let runtime = cx.runtime();
        runtime.gc().ensure_no_native_borrow()?;
        runtime.resources().ensure_execution_allowed()?;
        runtime.validate_loaded_module(ty.owner())?;
        cx.poll()?;
        let mut conversion = ConversionContext::new(runtime, ty.owner())?;
        let (_roots, values) = encode_arguments(&mut conversion, &record.payload, payload)?;
        let id = runtime.alloc_enum(EnumTag::Declared(record.member.layout.clone()), values)?;
        let root = runtime
            .root_value(Value::Enum(id))
            .ok_or_else(|| RuntimeError::module_validation("enum construction retention"))?;
        runtime.gc_safepoint()?;
        conversion.decode_prepared(
            ty.type_argument(),
            &root
                .value(runtime.gc())
                .ok_or_else(|| RuntimeError::module_validation("constructed enum value"))?,
        )
    }
}
