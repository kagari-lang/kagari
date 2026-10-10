//! Lexical call facts belong to a linked member and an exact environment identity.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge, application_key::ApplicationArguments, groups::OperationId,
    },
    frame::types::{TypeEnvironment, arguments::TypeArgument, operations::OperationBindings},
    module::LoadedModule,
};
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::ids::FunctionRef;
use kagari_types::ty::{NominalTy, Ty};
use std::{slice, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct InterfaceCallSite {
    pub(crate) function: FunctionRef,
    pub(crate) pc: usize,
}

/// These facts contain no receiver value and do not own a host root. Witness
/// edges participate in the linked program's bounded descriptor retention.
#[derive(Debug)]
pub(crate) struct ScopedInterfaceCall {
    pub(crate) interface: TypeArgument,
    pub(crate) arguments: ApplicationArguments,
    pub(crate) operations: OperationBindings,
    pub(crate) operation: Option<OperationId>,
    pub(crate) slot: usize,
}

impl ScopedInterfaceCall {
    pub(crate) fn interface_type(&self) -> &NominalTy<DefinitionId> {
        let Ty::Trait(interface) = self.interface.ty() else {
            unreachable!("prepared interface type");
        };
        interface
    }

    pub(crate) fn trace<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        self.operations.trace_metadata(pending);
        pending.extend(self.operation.map(MetadataEdge::Operation));
    }
}

impl Runtime {
    pub(crate) fn prepare_scoped_interface_call(
        &self,
        owner: &LoadedModule,
        environment: TypeEnvironment,
        site: InterfaceCallSite,
    ) -> Result<Arc<ScopedInterfaceCall>, RuntimeError> {
        self.validate_loaded_module(owner)?;
        let scope = environment.id;
        self.validate_environment(scope)?;
        if let Some(prepared) = self.modules.interface_call(owner, scope, site) {
            return Ok(prepared);
        }
        let prepared = self.build_interface_call(owner, Some(environment), site)?;
        self.publish_interface_call(owner, scope, site, prepared.clone())?;
        Ok(prepared)
    }

    pub(crate) fn build_interface_call(
        &self,
        owner: &LoadedModule,
        environment: Option<TypeEnvironment>,
        site: InterfaceCallSite,
    ) -> Result<Arc<ScopedInterfaceCall>, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::InterfaceCallPreparation);
        let invalid = || RuntimeError::module_validation("generic call operation environment");
        let BytecodeInstruction::Call {
            callee: CallTarget::InterfaceMethod { contract, .. },
            ..
        } = owner
            .bytecode
            .functions
            .get(site.function.index())
            .and_then(|function| function.instructions.get(site.pc))
            .ok_or_else(invalid)?
        else {
            return Err(invalid());
        };
        let types = environment
            .as_ref()
            .map(|environment| environment.types.clone());
        let interface = self
            .type_arguments(
                owner,
                types.clone(),
                slice::from_ref(&Ty::Trait(contract.interface.clone())),
            )?
            .pop()
            .ok_or_else(invalid)?;
        let Ty::Trait(interface_type) = interface.ty() else {
            return Err(invalid());
        };
        let operation = if let Some(receiver) = &contract.receiver {
            let environment = environment.as_ref().ok_or_else(invalid)?;
            let receiver = environment.types.resolve(receiver)?;
            Some(
                environment
                    .operation_slot(&self.gc, &receiver, interface_type, contract.method_slot)
                    .ok_or_else(invalid)?,
            )
        } else {
            None
        };
        let arguments = ApplicationArguments::new(
            owner,
            self.type_arguments(owner, types, &contract.arguments)?,
        )?;
        let operations = self.bind_operations_in(owner, environment, &contract.operations)?;
        let prepared = Arc::new(ScopedInterfaceCall {
            interface,
            arguments,
            operations,
            operation,
            slot: contract.method_slot as usize,
        });
        Ok(prepared)
    }
}
