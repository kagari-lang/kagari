use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::roots::RootedValue,
    module::{LoadedModule, retention::ProgramLease},
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{IntoKagari, KagariType, context::ConversionContext},
        types::Type,
    },
    value::Value,
};
use kagari_types::ty::Ty;

#[derive(Debug, Clone)]
pub(crate) struct AppliedReceiver {
    owner: LoadedModule,
    argument: TypeArgument,
    _program: ProgramLease,
}

impl AppliedReceiver {
    pub(crate) fn new(owner: LoadedModule, argument: TypeArgument, program: ProgramLease) -> Self {
        Self {
            owner,
            argument,
            _program: program,
        }
    }

    pub(crate) fn owner(&self) -> &LoadedModule {
        &self.owner
    }

    pub(crate) fn type_argument(&self) -> &TypeArgument {
        &self.argument
    }

    pub(crate) fn validate(&self, runtime: &Runtime) -> NativeResult<()> {
        runtime.validate_loaded_module(&self.owner)?;
        self.argument.validate(runtime)
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.owner.program_root().key() == other.owner.program_root().key()
            && self
                .argument
                .view(&self.owner)
                .compatible(other.argument.view(&other.owner))
    }
}

#[derive(Debug)]
pub(super) struct RetainedReceiver {
    pub(super) root: RootedValue,
    pub(super) applied: AppliedReceiver,
}

impl KagariType for RetainedReceiver {
    fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
        Err(RuntimeError::module_validation(
            "method receiver needs an installed type",
        ))
    }

    fn check_type(cx: &ConversionContext<'_>, expected: &TypeArgument) -> NativeResult<()> {
        expected.validate(cx.runtime())?;
        if matches!(expected.ty(), Ty::Struct(_) | Ty::NativeObject(_)) {
            Ok(())
        } else {
            Err(RuntimeError::module_validation("method receiver type"))
        }
    }
}

impl IntoKagari for RetainedReceiver {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        self.applied.validate(cx.runtime())?;
        if !self
            .applied
            .argument
            .view(&self.applied.owner)
            .compatible(expected.view(cx.owner()))
        {
            return Err(RuntimeError::module_validation("method receiver scope"));
        }
        let value = self
            .root
            .value(cx.runtime().gc())
            .ok_or_else(|| RuntimeError::module_validation("foreign or expired method receiver"))?;
        cx.check_value(expected, &value)?;
        Ok(value)
    }
}
