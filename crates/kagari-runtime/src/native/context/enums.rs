//! Native enum operations consume installed nominal layouts and scoped type arguments.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::{TypeArgument, variants::PreparedEnumMember},
    module::{EnumVariantRef, ExecutableModule},
    native::{
        binding::NativeResult,
        context::NativeCall,
        conversion::context::{ConversionContext, inputs::ValueInput},
        types::{Type, VariantRef},
    },
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_types::ty::Ty;
use std::slice;

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
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::NativeTypeMapping);
        let ty = ty.adopt(self.definition_context())?;
        let ty = owner
            .identity_admission()
            .product(&ty, &Default::default())?;
        self.resolve_type_arguments(owner, slice::from_ref(&ty))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("nominal type scope"))
    }

    /// Construct a declared member in a validated, generation-pinned type scope.
    /// Member lookup and payload checks precede allocation. Inputs and result
    /// carry explicit Host ownership; Native bindings use their call's source.
    pub fn make_enum_member(
        &mut self,
        owner: &ExecutableModule,
        applied: &TypeArgument,
        member: &str,
        fields: &[RootedValue],
    ) -> NativeResult<RootedValue> {
        let layout = self.declared_enum_variant(owner, applied, member)?;
        let mut conversion = ConversionContext::new(self, owner)?;
        let value = conversion.allocate_enum_inputs(layout, fields.iter().map(ValueInput::Root))?;
        conversion.retain(&value)
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

impl<'call> NativeCall<'call> {
    fn declared_enum_variant(
        &self,
        applied: &TypeArgument,
        variant: &VariantRef,
    ) -> NativeResult<EnumVariantRef> {
        self.runtime
            .checked_enum_variant(self.owner, applied, variant)
    }

    /// Allocate an ordinary registered enum in its checked, pinned type scope.
    /// The completed value transfers into this call's protected source suffix.
    pub fn allocate_enum(
        &mut self,
        applied: &TypeArgument,
        variant: &VariantRef,
        fields: &[ValueView<'_>],
    ) -> NativeResult<ValueView<'call>> {
        let layout = self.declared_enum_variant(applied, variant)?;
        self.convert_value(|conversion| {
            conversion.allocate_enum_inputs(layout, fields.iter().copied().map(ValueInput::View))
        })
    }

    /// Test a checked member of the argument's enum; foreign member handles fail.
    pub fn enum_argument_is(&mut self, index: usize, variant: &VariantRef) -> NativeResult<bool> {
        let function = self.function;
        let applied = function
            .type_signature()?
            .params
            .get(index)
            .ok_or_else(invalid_argument)?;
        let expected = self.declared_enum_variant(applied, variant)?;
        let value = self.argument(index)?;
        if !matches!(applied.ty(), Ty::Enum(_)) {
            return Err(invalid_argument());
        }
        let mut conversion = ConversionContext::in_native_call(self)?;
        conversion.check_value(applied, &value)?;
        let (actual, _) = conversion.enum_shape(&value)?;
        Ok(actual.matches_layout(&expected))
    }

    /// Read a checked payload field, protected through this Native call's suffix.
    pub fn enum_argument_field(
        &mut self,
        index: usize,
        variant: &VariantRef,
        field: usize,
    ) -> NativeResult<ValueView<'call>> {
        let function = self.function;
        let applied = function
            .type_signature()?
            .params
            .get(index)
            .ok_or_else(invalid_argument)?;
        let expected = self.declared_enum_variant(applied, variant)?;
        let value = self.argument(index)?;
        if !matches!(applied.ty(), Ty::Enum(_)) {
            return Err(invalid_argument());
        }
        self.convert_value(|conversion| {
            conversion.check_value(applied, &value)?;
            let (actual, count) = conversion.enum_shape(&value)?;
            if !actual.matches_layout(&expected) {
                return Err(RuntimeError::module_validation(
                    "native enum argument variant",
                ));
            }
            if field >= count {
                return Err(RuntimeError::module_validation("native enum payload index"));
            }
            conversion.enum_element(&value, field)
        })
    }
}

fn invalid_argument() -> RuntimeError {
    RuntimeError::module_validation("native enum argument type")
}
