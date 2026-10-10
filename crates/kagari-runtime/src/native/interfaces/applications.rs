//! Method applications reuse closed call witnesses from the installed program.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::MetadataRoot,
    frame::types::{
        EnvironmentRecord,
        arguments::{ScopedSignature, TypeArgument},
        compatibility::TypeView,
        operations::OperationBindings,
    },
    gc::roots::RootSet,
    module::{LoadedModule, ModuleKey},
    native::{binding::NativeResult, interfaces::binding::InterfaceMember},
};
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{callable::interface::InterfaceCallContract, ids::FunctionRef};
use kagari_types::ty::Ty;
use std::slice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ApplicationKey(ModuleKey, FunctionRef, usize);

pub(super) struct ApplicationEvidence {
    pub(super) key: ApplicationKey,
    owner: LoadedModule,
    contract: InterfaceCallContract<DefinitionId>,
}

#[derive(Debug)]
pub(super) struct AppliedMethod {
    pub(super) key: ApplicationKey,
    pub(super) arguments: Vec<TypeArgument>,
    pub(super) operations: OperationBindings,
    _roots: RootSet,
}

fn missing() -> RuntimeError {
    RuntimeError::module_validation("generic interface method has no checked closed call witness")
}

impl ApplicationEvidence {
    pub(super) fn find(
        runtime: &Runtime,
        member: &InterfaceMember,
        arguments: &[TypeArgument],
    ) -> NativeResult<Self> {
        for argument in arguments {
            argument.validate(runtime)?;
        }
        for owner in member.owner.members() {
            for function in &owner.bytecode.functions {
                if function.metadata.semantic.generic.is_some() {
                    continue;
                }
                for (offset, instruction) in function.instructions.iter().enumerate() {
                    let BytecodeInstruction::Call {
                        callee: CallTarget::InterfaceMethod { contract, .. },
                        ..
                    } = instruction
                    else {
                        continue;
                    };
                    let interface = Ty::Trait(contract.interface.clone());
                    if contract.receiver.is_some()
                        || contract.method_slot as usize != member.slot
                        || &interface != member.interface.ty()
                        || !interface.is_concrete()
                        || contract.arguments.len() != arguments.len()
                        || !member
                            .interface
                            .view(&member.owner)
                            .compatible(TypeView::new(&interface, &owner, None))
                        || !contract
                            .arguments
                            .iter()
                            .zip(arguments)
                            .all(|(ty, argument)| {
                                ty.is_concrete()
                                    && ty == argument.ty()
                                    && argument
                                        .view(&member.owner)
                                        .compatible(TypeView::new(ty, &owner, None))
                            })
                    {
                        continue;
                    }
                    return Ok(Self {
                        key: ApplicationKey(owner.key(), function.id, offset),
                        owner: owner.clone(),
                        contract: *contract.clone(),
                    });
                }
            }
        }
        Err(missing())
    }

    pub(super) fn signature(
        &self,
        runtime: &Runtime,
        member: &InterfaceMember,
    ) -> NativeResult<ScopedSignature> {
        let catalog = &runtime.native_entries.catalog;
        let call = catalog.paths(&self.contract)?;
        let signature = call
            .signature(&member.contract, &Default::default())
            .map_err(|_| missing())?;
        let params = catalog.scope(&signature.params)?;
        let result = catalog.scope(&signature.result)?;
        Ok(ScopedSignature {
            params: runtime
                .resolve_type_arguments(&self.owner, params.get(1..).ok_or_else(missing)?)?,
            result: runtime
                .resolve_type_arguments(&self.owner, slice::from_ref(&result))?
                .pop()
                .ok_or_else(missing)?,
        })
    }

    pub(super) fn prepare(
        &self,
        runtime: &Runtime,
        member: &InterfaceMember,
    ) -> NativeResult<AppliedMethod> {
        let arguments = runtime.resolve_type_arguments(&self.owner, &self.contract.arguments)?;
        let parameters = runtime
            .native_entries
            .catalog
            .scope(&member.contract.methods[member.slot].generic_params)?;
        let operations =
            runtime.bind_operations_in(&self.owner, None, &self.contract.operations)?;
        let mut environment =
            EnvironmentRecord::new(runtime.definition_context(), parameters, arguments.clone())?;
        environment.extend_operations(operations.clone());
        let environment = runtime.alloc_environment(environment)?;
        let roots = runtime.root_metadata(vec![
            MetadataRoot::Program(self.owner.clone()),
            MetadataRoot::Environment(environment.id),
        ])?;
        Ok(AppliedMethod {
            key: self.key,
            arguments,
            operations,
            _roots: roots,
        })
    }
}
