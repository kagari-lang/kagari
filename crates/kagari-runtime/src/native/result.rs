//! Execute a compiler-selected native result conversion without trait lookup.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{TypeEnvironment, arguments::TypeArgument},
    module::{LoadedModule, VerifiedProgram},
    native::binding::NativeResult,
    value::Value,
};
use kagari_bytecode::{program::ModuleRef, trait_bounds::views::native_result_target};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::native_import::NativeImport;
use kagari_types::ty::Ty;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub(crate) struct LinkedResultAdapter {
    owner: ModuleRef,
    table: usize,
    arguments: Vec<Ty<DefinitionId>>,
    applied: Option<Vec<TypeArgument>>,
}

impl LinkedResultAdapter {
    pub(crate) fn link(
        import: &NativeImport<DefinitionId>,
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
        }))
    }

    pub(crate) fn apply(
        &self,
        runtime: &Runtime,
        owner: &LoadedModule,
        environment: Rc<TypeEnvironment>,
    ) -> NativeResult<Self> {
        Ok(Self {
            applied: Some(runtime.type_arguments(owner, Some(environment), &self.arguments)?),
            ..self.clone()
        })
    }

    pub(crate) fn convert(
        &self,
        runtime: &Runtime,
        owner: &LoadedModule,
        value: Value,
    ) -> NativeResult<Value> {
        let _root = runtime
            .root_value(value.clone())
            .ok_or_else(|| RuntimeError::module_validation("native result root"))?;
        let resolved;
        let arguments = match &self.applied {
            Some(arguments) => arguments,
            None => {
                resolved = runtime.resolve_type_arguments(owner, &self.arguments)?;
                &resolved
            }
        };
        let implementation = owner
            .member(self.owner)
            .ok_or_else(|| RuntimeError::module_validation("native result owner"))?;
        runtime.make_interface_applied(&implementation, self.table, arguments, value)
    }
}
