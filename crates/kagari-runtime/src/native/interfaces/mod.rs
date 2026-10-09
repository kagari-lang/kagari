//! Retained dynamic interface views preserve the declared access and lexical scope.
mod applications;
pub mod binding;
mod calls;
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::roots::RootedValue,
    module::{LoadedModule, ModuleEpochRetention, retention::ProgramLease},
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        types::Type,
    },
    value::Value,
};
use kagari_types::ty::Ty;
use std::marker::PhantomData;

/// The installed signature supplies the precise trait application. This marker
/// does not introduce an erased script type or make unrelated traits compatible.
#[derive(Debug)]
pub struct DynamicInterface;

/// An owning host view of an existing interface value. Clones preserve aliases,
/// its dispatch table, associated outputs and the original implementation version.
#[derive(Debug)]
pub struct Interface<S = DynamicInterface> {
    root: RootedValue,
    argument: TypeArgument,
    owner: LoadedModule,
    program: ProgramLease,
    schema: PhantomData<fn() -> S>,
}

impl<S> Clone for Interface<S> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            argument: self.argument.clone(),
            owner: self.owner.clone(),
            program: self.program.clone(),
            schema: PhantomData,
        }
    }
}

impl<S> Interface<S> {
    pub fn type_argument(&self) -> &TypeArgument {
        &self.argument
    }

    pub fn owner(&self) -> &LoadedModule {
        &self.owner
    }
}

impl KagariType for DynamicInterface {
    fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
        Err(RuntimeError::module_validation(
            "dynamic interface requires an installed trait signature",
        ))
    }

    fn check_type(_: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        if matches!(expected.ty(), Ty::Trait(_)) {
            Ok(())
        } else {
            Err(RuntimeError::module_validation(
                "interface conversion requires a trait type",
            ))
        }
    }
}

impl<S: KagariType> KagariType for Interface<S> {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        let ty = S::kagari_type(catalog)?;
        if !matches!(ty.abi(), Ty::Trait(_)) {
            return Err(RuntimeError::module_validation(
                "interface schema requires a trait type",
            ));
        }
        Ok(ty)
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        DynamicInterface::check_type(cx, expected)?;
        S::check_type(cx, expected)
    }
}

impl<S: KagariType> FromKagari for Interface<S> {
    const PRESERVES_IDENTITY: bool = true;

    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
    ) -> NativeResult<Self> {
        cx.check_type::<Self>(expected)?;
        cx.check_value(expected, value)?;
        if !matches!(value, Value::Interface(_)) {
            return Err(RuntimeError::module_validation(
                "interface value requires an installed dispatch table",
            ));
        }
        let root = cx
            .runtime()
            .root_value(*value)
            .ok_or_else(|| RuntimeError::module_validation("interface handle retention"))?;
        // The dispatch snapshot retains its implementation. The declared view
        // can have a different lexical owner; retain that scope independently.
        let program = cx
            .runtime()
            .retain_program(cx.owner(), ModuleEpochRetention::RuntimeValue)
            .ok_or_else(|| RuntimeError::module_validation("interface type scope retention"))?;
        Ok(Self {
            root,
            argument: expected.clone(),
            owner: cx.owner().clone(),
            program,
            schema: PhantomData,
        })
    }
}

impl<S: KagariType> IntoKagari for Interface<S> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        cx.check_type::<Self>(expected)?;
        if !self
            .argument
            .view(&self.owner)
            .compatible(expected.view(cx.owner()))
        {
            return Err(RuntimeError::module_validation(
                "interface view differs from its declared application",
            ));
        }
        let value = self.root.value(cx.runtime().gc()).ok_or_else(|| {
            RuntimeError::module_validation("foreign or expired interface handle")
        })?;
        cx.check_value(expected, &value)?;
        Ok(value)
    }
}
