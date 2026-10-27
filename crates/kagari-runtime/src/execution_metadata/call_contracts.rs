//! Lexical call facts belong to a linked member and an exact environment identity.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge, application_key::ApplicationArguments, groups::OperationId,
        scan_records::InterfaceCallScanId,
    },
    frame::types::{
        TypeEnvironment, arguments::TypeArgument, compatibility::TypeView,
        operations::OperationBindings,
    },
    module::{ExecutableModule, linked_execution::calls::LinkedCall},
};
use kagari_contract::{callable::interface::InterfaceCallContract, ids::FunctionRef};
use kagari_types::ty::{NominalTy, Ty};
use std::{slice, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct InterfaceCallSite {
    pub(crate) function: FunctionRef,
    pub(crate) ordinal: usize,
}

/// These facts contain no receiver value and do not own a host root. Witness
/// edges participate in the linked program's bounded descriptor retention.
#[derive(Debug)]
pub(crate) struct ScopedInterfaceCall {
    pub(crate) scan_id: InterfaceCallScanId,
    pub(crate) interface: TypeArgument,
    pub(crate) arguments: ApplicationArguments,
    pub(crate) operations: OperationBindings,
    pub(crate) operation: Option<OperationId>,
    pub(crate) slot: usize,
}

impl ScopedInterfaceCall {
    pub(crate) fn interface_type(&self) -> &NominalTy {
        let Ty::Trait(interface) = self.interface.ty() else {
            unreachable!("prepared interface type");
        };
        interface
    }

    pub(crate) fn scan_len(&self) -> usize {
        1 + self.operations.scan_len() + self.arguments.constructions().scan_len()
    }

    pub(crate) fn scan_field(&self, index: usize) -> Option<MetadataEdge<'_>> {
        if index == 0 {
            return self.operation.map(MetadataEdge::Operation);
        }
        let index = index - 1;
        if index < self.operations.scan_len() {
            self.operations.scan_field(index)
        } else {
            self.arguments
                .constructions()
                .scan_field(index - self.operations.scan_len())
        }
    }

    pub(crate) fn trace<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        pending.extend((0..self.scan_len()).filter_map(|index| self.scan_field(index)));
    }
}

impl Runtime {
    pub(crate) fn prepare_scoped_interface_call(
        &self,
        owner: &ExecutableModule,
        environment: TypeEnvironment,
        site: InterfaceCallSite,
    ) -> Result<Arc<ScopedInterfaceCall>, RuntimeError> {
        self.validate_executable_module(owner)?;
        let scope = environment.id;
        self.validate_environment(scope)?;
        if let Some(prepared) = self.modules.interface_call(&self.gc, owner, scope, site) {
            return Ok(prepared);
        }
        let invalid = || RuntimeError::module_validation("invalid linked interface contract");
        let links = self
            .modules
            .linked_function(owner, site.function, Some(scope))
            .ok_or_else(invalid)?;
        let Some(LinkedCall::Interface { contract, .. }) = links.call_contract(site.ordinal) else {
            return Err(invalid());
        };
        let prepared = self.build_interface_call(owner, Some(environment), contract)?;
        self.publish_interface_call(owner, scope, site, prepared.clone())?;
        Ok(prepared)
    }

    pub(crate) fn build_interface_call(
        &self,
        owner: &ExecutableModule,
        environment: Option<TypeEnvironment>,
        contract: &InterfaceCallContract,
    ) -> Result<Arc<ScopedInterfaceCall>, RuntimeError> {
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::InterfaceCallPreparation);
        let invalid = || RuntimeError::module_validation("generic call operation environment");
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
            let receiver = TypeView::new(receiver, owner, Some(environment.types.as_ref()))
                .identity()
                .ok_or_else(invalid)?;
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
            self.bind_constructions_in(owner, environment.clone(), &contract.constructions)?,
        )?;
        let operations = self.bind_operations_in(owner, environment, &contract.operations)?;
        let prepared =
            self.gc
                .admit_interface_call(self.modules(), |scan_id| ScopedInterfaceCall {
                    scan_id,
                    interface,
                    arguments,
                    operations,
                    operation,
                    slot: contract.method_slot as usize,
                })?;
        Ok(prepared)
    }
}
