//! Native enum operations consume installed nominal layouts and scoped type arguments.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::{TypeArgument, variants::PreparedEnumMember},
    module::{EnumVariantRef, ExecutableModule},
    native::{
        binding::NativeResult,
        context::CallContext,
        types::{Type, VariantRef},
    },
    value::{EnumTag, EnumValueSnapshot, Value},
};
use kagari_types::ty::Ty;
use std::{cell::Ref, slice};

impl Runtime {
    fn declared_enum_member<'a>(
        &self,
        fallback: &ExecutableModule,
        applied: &'a TypeArgument,
        member: &str,
    ) -> NativeResult<&'a PreparedEnumMember> {
        self.validate_executable_module(fallback)?;
        applied.validate(self)?;
        applied
            .prepared_variants(|| self.prepare_enum_variants(fallback, applied))?
            .iter()
            .find(|prepared| prepared.name() == Some(member))
            .ok_or_else(|| RuntimeError::module_validation("variant belongs to another enum"))
    }

    pub(crate) fn declared_enum_variant(
        &self,
        fallback: &ExecutableModule,
        applied: &TypeArgument,
        member: &str,
    ) -> NativeResult<EnumVariantRef> {
        self.declared_enum_member(fallback, applied, member)
            .map(|prepared| prepared.layout().clone())
    }

    pub(crate) fn native_type_argument(
        &self,
        owner: &ExecutableModule,
        ty: &Type,
    ) -> NativeResult<TypeArgument> {
        let ty = ty.adopt(self.definition_context())?;
        self.resolve_type_arguments(owner, slice::from_ref(&ty))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("nominal type scope"))
    }

    /// Construct a declared member in a validated, generation-pinned type scope.
    /// Member lookup and payload checks precede allocation; retain a root before
    /// another allocation or reentry. Native bindings normally use VariantRef.
    pub fn make_enum_member(
        &self,
        owner: &ExecutableModule,
        applied: &TypeArgument,
        member: &str,
        fields: Vec<Value>,
    ) -> NativeResult<Value> {
        let layout = self.declared_enum_variant(owner, applied, member)?;
        self.alloc_enum(EnumTag::Declared(layout), fields)
            .map(Value::Enum)
    }
}

impl Runtime {
    pub(crate) fn checked_enum_variant(
        &self,
        owner: &ExecutableModule,
        applied: &TypeArgument,
        variant: &VariantRef,
    ) -> NativeResult<EnumVariantRef> {
        let identity = variant
            .definitions()
            .resolve(variant.id())
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        let member = identity
            .last()
            .ok_or_else(|| RuntimeError::module_validation("native enum member identity"))?;
        let prepared = self.declared_enum_member(owner, applied, member.name)?;
        if !prepared.matches_identity(variant.id(), variant.definitions()) {
            return Err(RuntimeError::module_validation(
                "variant belongs to another enum",
            ));
        }
        Ok(prepared.layout().clone())
    }
}

impl CallContext<'_> {
    fn declared_enum_variant(
        &self,
        applied: &TypeArgument,
        variant: &VariantRef,
    ) -> NativeResult<EnumVariantRef> {
        self.runtime
            .checked_enum_variant(self.owner, applied, variant)
    }

    /// Allocate an ordinary registered enum in its checked, pinned type scope.
    /// Root the returned Value before further allocation or synchronous reentry.
    pub fn allocate_enum(
        &self,
        applied: &TypeArgument,
        variant: &VariantRef,
        fields: Vec<Value>,
    ) -> NativeResult<Value> {
        let layout = self.declared_enum_variant(applied, variant)?;
        self.runtime
            .alloc_enum(EnumTag::Declared(layout), fields)
            .map(Value::Enum)
    }

    fn enum_argument_view(&self, index: usize) -> NativeResult<Ref<'_, EnumValueSnapshot>> {
        let value = self.argument(index)?;
        let applied = self.argument_type_argument(index)?;
        if !matches!(applied.ty(), Ty::Enum(_))
            || !applied.matches(self.runtime, &value, self.owner)
        {
            return Err(RuntimeError::module_validation("native enum argument type"));
        }
        let Value::Enum(handle) = value else {
            return Err(RuntimeError::module_validation(
                "native enum argument value",
            ));
        };
        self.heap()
            .enum_view(handle)
            .ok_or_else(|| RuntimeError::module_validation("native enum argument handle"))
    }

    /// Test a checked member of the argument's enum; foreign member handles fail.
    pub fn enum_argument_is(&self, index: usize, variant: &VariantRef) -> NativeResult<bool> {
        let expected = self.declared_enum_variant(&self.argument_type_argument(index)?, variant)?;
        let view = self.enum_argument_view(index)?;
        Ok(matches!(&view.tag, EnumTag::Declared(actual) if actual.matches_layout(&expected)))
    }

    /// Read a checked payload field; the argument root retains its referent for this call.
    pub fn enum_argument_field(
        &self,
        index: usize,
        variant: &VariantRef,
        field: usize,
    ) -> NativeResult<Value> {
        let expected = self.declared_enum_variant(&self.argument_type_argument(index)?, variant)?;
        let view = self.enum_argument_view(index)?;
        if !matches!(&view.tag, EnumTag::Declared(actual) if actual.matches_layout(&expected)) {
            return Err(RuntimeError::module_validation(
                "native enum argument variant",
            ));
        }
        view.fields
            .get(field)
            .copied()
            .ok_or_else(|| RuntimeError::module_validation("native enum payload index"))
    }
}
