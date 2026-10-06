//! Native enum operations consume installed nominal layouts and scoped type arguments.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, bindings::TypeBindings, compatibility::TypeView},
    module::{EnumVariantRef, LoadedModule},
    native::{binding::NativeResult, context::CallContext, types::VariantRef},
    value::{EnumTag, EnumValueSnapshot, Value},
};
use kagari_common::identity::{
    mapping::{DefinitionMapper, DefinitionRecord},
    table::DefinitionId,
};
use kagari_types::ty::{GenericParam, Ty};
use std::{slice, sync::Arc};

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

    fn prepare_enum_variants(
        &self,
        fallback: &LoadedModule,
        applied: &TypeArgument,
    ) -> NativeResult<Vec<EnumVariantRef>> {
        let Ty::Enum(nominal) = applied.ty() else {
            return Err(RuntimeError::module_validation(
                "native operation requires an enum type",
            ));
        };
        let view = applied
            .view(fallback)
            .normalized()
            .ok_or_else(|| RuntimeError::module_validation("native enum type scope"))?;
        let (owner, id) = view.owner.find_enum_definition(nominal).ok_or_else(|| {
            RuntimeError::module_validation("native enum layout is absent from the pinned program")
        })?;
        let template = &owner.bytecode.enumerations[id.index()];
        if template.variants.is_empty() {
            return Ok(Vec::new());
        }
        let mut layout = self
            .modules
            .applied_enum_variant(&owner, id, &nominal.arguments, 0)
            .ok_or_else(|| RuntimeError::module_validation("native enum layout application"))?;
        let arguments = (0..nominal.arguments.len())
            .map(|position| applied.parameter(self, fallback, position))
            .collect::<NativeResult<Vec<_>>>()?;
        if arguments.iter().any(TypeArgument::has_origin) {
            if !template.arguments.iter().enumerate().all(|(position, ty)| {
                matches!(ty, Ty::Parameter { owner, position: slot } if *owner == nominal.declaration && *slot == position)
            }) {
                if template.arguments.iter().zip(&arguments).all(|(compiled, supplied)| {
                    compiled.is_concrete() && supplied.view(fallback).compatible(TypeView::new(compiled, &owner, None))
                }) {
                    return enum_variants(&layout);
                }
                return Err(RuntimeError::module_validation("scoped enum payload differs from its concrete layout"));
            }
            let parameters = (0..arguments.len())
                .map(|position| GenericParam {
                    owner: nominal.declaration,
                    position,
                })
                .collect();
            layout.environment = Some(Arc::new(TypeBindings::new(
                self.definition_context(),
                parameters,
                arguments,
            )?));
        }
        enum_variants(&layout)
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

    fn enum_argument_snapshot(&self, index: usize) -> NativeResult<EnumValueSnapshot> {
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
            .enum_snapshot(handle)
            .ok_or_else(|| RuntimeError::module_validation("native enum argument handle"))
    }

    /// Test a checked member of the argument's enum; foreign member handles fail.
    pub fn enum_argument_is(&self, index: usize, variant: &VariantRef) -> NativeResult<bool> {
        let expected = self.declared_enum_variant(&self.argument_type_argument(index)?, variant)?;
        let snapshot = self.enum_argument_snapshot(index)?;
        Ok(matches!(snapshot.tag, EnumTag::Declared(actual) if actual.matches_layout(&expected)))
    }

    /// Read a checked payload field; the argument root retains its referent for this call.
    pub fn enum_argument_field(
        &self,
        index: usize,
        variant: &VariantRef,
        field: usize,
    ) -> NativeResult<Value> {
        let expected = self.declared_enum_variant(&self.argument_type_argument(index)?, variant)?;
        let snapshot = self.enum_argument_snapshot(index)?;
        if !matches!(snapshot.tag, EnumTag::Declared(actual) if actual.matches_layout(&expected)) {
            return Err(RuntimeError::module_validation(
                "native enum argument variant",
            ));
        }
        snapshot
            .fields
            .get(field)
            .cloned()
            .ok_or_else(|| RuntimeError::module_validation("native enum payload index"))
    }
}

fn enum_variants(layout: &EnumVariantRef) -> NativeResult<Vec<EnumVariantRef>> {
    (0..layout.layout().variants.len())
        .map(|index| {
            u32::try_from(index)
                .ok()
                .and_then(|index| layout.with_variant(index))
                .ok_or_else(|| RuntimeError::module_validation("enum variant index"))
        })
        .collect()
}
