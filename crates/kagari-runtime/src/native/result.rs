//! Execute a compiler-selected native result conversion without trait lookup.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{TypeEnvironment, arguments::TypeArgument},
    module::{ExecutableModule, VerifiedProgram},
    native::{binding::NativeResult, conversion::context::ConversionContext},
    objects::interfaces::InterfaceRequest,
    value::view::ValueView,
};
use kagari_bytecode::{program::ModuleRef, trait_bounds::views::native_result_target};
use kagari_contract::native_import::NativeImport;
use kagari_types::ty::Ty;

#[derive(Debug, Clone)]
pub(crate) struct LinkedResultAdapter {
    owner: ModuleRef,
    table: usize,
    arguments: Vec<Ty>,
    applied: Option<Vec<TypeArgument>>,
    environment: Option<TypeEnvironment>,
}

impl LinkedResultAdapter {
    pub(crate) fn link(
        import: &NativeImport,
        program: &VerifiedProgram,
    ) -> NativeResult<Option<Self>> {
        let Some(adapter) = &import.result_adapter else {
            return Ok(None);
        };
        let closure = program
            .modules()
            .iter()
            .map(|module| module.as_ref())
            .collect::<Vec<_>>();
        let (owner, table) = native_result_target(adapter, &closure)
            .ok_or_else(|| RuntimeError::module_validation("native result table is absent"))?;
        Ok(Some(Self {
            owner,
            table,
            arguments: adapter.implementation.arguments.clone(),
            applied: None,
            environment: None,
        }))
    }

    pub(crate) fn apply(
        &self,
        runtime: &Runtime,
        owner: &ExecutableModule,
        environment: TypeEnvironment,
    ) -> NativeResult<Self> {
        Ok(Self {
            applied: Some(runtime.type_arguments(
                owner,
                Some(environment.types.clone()),
                &self.arguments,
            )?),
            environment: Some(environment),
            ..self.clone()
        })
    }

    pub(crate) fn convert<'call>(
        &self,
        cx: &mut ConversionContext<'call>,
        value: ValueView<'_>,
    ) -> NativeResult<ValueView<'call>> {
        let owner = cx.owner();
        let resolved;
        let arguments = match &self.applied {
            Some(arguments) => arguments,
            None => {
                resolved = cx
                    .runtime()
                    .resolve_type_arguments(owner, &self.arguments)?;
                &resolved
            }
        };
        let implementation = owner
            .member(self.owner)
            .ok_or_else(|| RuntimeError::module_validation("native result owner"))?;
        cx.make_interface(
            InterfaceRequest {
                implementation: &implementation,
                table: self.table,
                arguments,
                environment: self.environment.as_ref(),
                view: false,
            },
            &value,
        )
    }
}
