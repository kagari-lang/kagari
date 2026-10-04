//! Native enum operations consume installed nominal layouts and scoped type arguments.
use crate::{
    error::RuntimeError,
    frame::types::{TypeEnvironment, arguments::TypeArgument},
    module::EnumVariantRef,
    native::{binding::NativeResult, context::CallContext, types::VariantRef},
    value::{EnumTag, EnumValueSnapshot, Value},
};
use kagari_types::ty::{GenericParam, Ty};
use std::rc::Rc;

impl CallContext<'_> {
    fn declared_enum_variant(
        &self,
        applied: &TypeArgument,
        variant: &VariantRef,
    ) -> NativeResult<EnumVariantRef> {
        applied.validate(self.runtime)?;
        let Ty::Enum(nominal) = applied.ty() else {
            return Err(RuntimeError::module_validation(
                "native operation requires an enum type",
            ));
        };
        let view = applied
            .view(self.owner)
            .normalized()
            .ok_or_else(|| RuntimeError::module_validation("native enum type scope"))?;
        let (owner, id) = view.owner.find_enum_definition(nominal).ok_or_else(|| {
            RuntimeError::module_validation("native enum layout is absent from the pinned program")
        })?;
        let declaration = owner.definitions().lookup(variant.id()).ok_or_else(|| {
            RuntimeError::module_validation("native enum variant is absent from the pinned program")
        })?;
        let template = &owner.bytecode.enumerations[id.index()];
        let index = template
            .variants
            .iter()
            .position(|member| member.declaration == declaration)
            .ok_or_else(|| RuntimeError::module_validation("variant belongs to another enum"))?;
        let index = u32::try_from(index)
            .map_err(|_| RuntimeError::module_validation("enum variant index"))?;
        let mut layout = owner
            .applied_enum_variant(id, &nominal.arguments, index)
            .ok_or_else(|| RuntimeError::module_validation("native enum layout application"))?;
        let arguments = (0..nominal.arguments.len())
            .map(|position| applied.parameter(self.runtime, self.owner, position))
            .collect::<NativeResult<Vec<_>>>()?;
        if arguments.iter().any(TypeArgument::has_origin) {
            if !template.arguments.iter().enumerate().all(|(position, ty)| {
                matches!(ty, Ty::Parameter { owner, position: slot } if *owner == nominal.declaration && *slot == position)
            }) {
                return Err(RuntimeError::module_validation("scoped enum payload requires its declaration template"));
            }
            let parameters = (0..arguments.len())
                .map(|position| GenericParam {
                    owner: nominal.declaration,
                    position,
                })
                .collect();
            layout.environment = Some(Rc::new(TypeEnvironment::new(
                self.runtime.definition_context(),
                parameters,
                arguments,
            )?));
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
