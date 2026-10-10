//! Native enum operations consume installed nominal layouts and scoped type arguments.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::{EnumVariantRef, LoadedModule},
    native::{binding::NativeResult, context::CallContext, types::VariantRef},
    value::{EnumTag, EnumValueSnapshot, Value},
};
use kagari_common::identity::{
    mapping::{DefinitionMapper, DefinitionRecord},
    table::DefinitionId,
};
use kagari_types::ty::Ty;
use std::{cell::Ref, slice};

impl Runtime {
    pub(crate) fn declared_enum_variant(
        &self,
        fallback: &LoadedModule,
        applied: &TypeArgument,
        member: &str,
    ) -> NativeResult<EnumVariantRef> {
        self.validate_loaded_module(fallback)?;
        applied.validate(self)?;
        applied
            .prepared_variants(|| self.prepare_enum_variants(fallback, applied))?
            .iter()
            .find(|layout| {
                layout
                    .module()
                    .definition_name(layout.variant().declaration)
                    == Some(member)
            })
            .cloned()
            .ok_or_else(|| RuntimeError::module_validation("variant belongs to another enum"))
    }

    pub(crate) fn portable_type_argument(
        &self,
        owner: &LoadedModule,
        ty: &Ty,
    ) -> NativeResult<TypeArgument> {
        let ty: Ty<DefinitionId> = ty
            .map_identities(&mut DefinitionMapper::new(
                &mut |path| self.definition_context().intern(path).map_err(Into::into),
                &Default::default(),
            ))
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?;
        self.resolve_type_arguments(owner, slice::from_ref(&ty))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("nominal type scope"))
    }

    /// Construct a declared member in a validated, generation-pinned type scope.
    /// Member lookup and payload checks precede allocation; retain a root before
    /// another allocation or reentry. Native bindings normally use VariantRef.
    pub fn make_enum_member(
        &self,
        owner: &LoadedModule,
        applied: &TypeArgument,
        member: &str,
        fields: Vec<Value>,
    ) -> NativeResult<Value> {
        let layout = self.declared_enum_variant(owner, applied, member)?;
        self.alloc_enum(EnumTag::Declared(layout), fields)
            .map(Value::Enum)
    }
}

impl CallContext<'_> {
    fn declared_enum_variant(
        &self,
        applied: &TypeArgument,
        variant: &VariantRef,
    ) -> NativeResult<EnumVariantRef> {
        let member = variant
            .id()
            .path
            .last()
            .ok_or_else(|| RuntimeError::module_validation("native enum member identity"))?;
        let layout = self
            .runtime
            .declared_enum_variant(self.owner, applied, &member.name)?;
        let expected = layout
            .module()
            .definitions()
            .lookup(variant.id())
            .ok_or_else(|| RuntimeError::module_validation("native enum member identity"))?;
        if expected != layout.variant().declaration {
            return Err(RuntimeError::module_validation(
                "variant belongs to another enum",
            ));
        }
        Ok(layout)
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
