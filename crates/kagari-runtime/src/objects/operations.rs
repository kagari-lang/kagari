//! Prepare receiver operations from verified table slots, never trait search.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{BoundGenericMethod, BoundOperation, TypeEnvironment},
    gc::interfaces::InterfaceResultBinding,
    module::LoadedModule,
    objects::calls,
};
use kagari_abi::{
    callable::witness::OperationWitness,
    native_import::{NativeSignature, callables::NativeCallableRequirement},
    types::{self as abi, AbiType, PublicAbiItem},
};
use kagari_bytecode::module::CallableTarget;
use std::rc::Rc;

impl Runtime {
    pub(crate) fn bind_receiver_operations(
        &self,
        owner: &LoadedModule,
        target: CallableTarget,
        binding: &InterfaceResultBinding,
    ) -> Result<Vec<BoundOperation>, RuntimeError> {
        let required = match target {
            CallableTarget::Native(target) => owner
                .bytecode
                .native_imports
                .get(target.index())
                .is_some_and(|import| {
                    import.callables.iter().any(|operation| {
                        matches!(operation, OperationWitness::Forward(required)
                    if import.signature.params.first() == Some(&required.receiver))
                    })
                }),
            CallableTarget::Script(_) => false,
        };
        if required {
            self.bind_table_operations(binding)
        } else {
            Ok(vec![])
        }
    }

    pub(crate) fn bind_table_operations(
        &self,
        binding: &InterfaceResultBinding,
    ) -> Result<Vec<BoundOperation>, RuntimeError> {
        let invalid = || RuntimeError::module_validation("receiver operation table");
        let mut pending = vec![binding.clone()];
        let mut operations: Vec<BoundOperation> = vec![];
        let mut visited = vec![];
        while let Some(binding) = pending.pop() {
            let owner = &binding.owner;
            self.validate_loaded_module(owner)?;
            let table = owner
                .bytecode
                .interface_tables
                .get(binding.table)
                .ok_or_else(invalid)?;
            let template = owner
                .bytecode
                .public_items
                .iter()
                .find_map(|item| match item {
                    PublicAbiItem::InterfaceTable(abi) if abi.declaration == table.declaration => {
                        Some(abi)
                    }
                    _ => None,
                })
                .ok_or_else(invalid)?;
            let arguments =
                self.type_arguments(owner, binding.environment.clone(), &binding.arguments)?;
            let key = (
                owner.key(),
                binding.table,
                arguments
                    .iter()
                    .map(|argument| argument.ty().clone())
                    .collect::<Vec<_>>(),
            );
            if visited.contains(&key) {
                continue;
            }
            visited.push(key);
            let environment = Rc::new(TypeEnvironment::new(
                template.generic_params.clone(),
                arguments,
            )?);
            let receiver = environment.resolve(&template.for_type)?;
            let AbiType::Trait(interface) = environment.resolve(&template.trait_type)? else {
                return Err(invalid());
            };
            let contract = owner
                .members()
                .find_map(|member| {
                    abi::trait_contract(
                        &member.bytecode.identity,
                        &member.bytecode.public_items,
                        &member.bytecode.trait_contracts,
                        &interface.declaration,
                    )
                    .cloned()
                })
                .ok_or_else(invalid)?;
            let retention = Rc::new(
                self.modules
                    .retain_runtime_program(owner)
                    .ok_or_else(invalid)?,
            );
            for slot in &table.methods {
                let name = &slot.method.path.last().ok_or_else(invalid)?.name;
                let method = template
                    .methods
                    .iter()
                    .find(|method| method.name == *name)
                    .ok_or_else(invalid)?;
                if method
                    .params
                    .first()
                    .is_none_or(|parameter| parameter.name != "self")
                {
                    continue;
                }
                let ordinal = contract
                    .methods
                    .iter()
                    .position(|method| method.name == *name)
                    .ok_or_else(invalid)?;
                let requirement = NativeCallableRequirement {
                    receiver: receiver.clone(),
                    interface: interface.clone(),
                    member: slot.method.clone(),
                    arguments: vec![],
                };
                if operations
                    .iter()
                    .any(|operation| operation.requirement == requirement)
                {
                    continue;
                }
                let body = match slot.target {
                    CallableTarget::Script(target) => owner
                        .bytecode
                        .functions
                        .get(target.index())
                        .ok_or_else(invalid)?
                        .metadata
                        .semantic
                        .generic
                        .as_ref(),
                    CallableTarget::Native(target) => owner
                        .bytecode
                        .native_imports
                        .get(target.index())
                        .ok_or_else(invalid)?
                        .generic
                        .as_ref(),
                };
                operations.push(BoundOperation {
                    generic: Some(BoundGenericMethod {
                        receiver_table: binding.clone(),
                        receiver_environment: Some(environment.clone()),
                        parameters: method.generic_params.clone(),
                        entry_parameters: body
                            .map(|body| body.parameters.clone())
                            .unwrap_or_default(),
                        entry_arguments: slot.arguments.clone(),
                    }),
                    requirement,
                    slot: ordinal as u32,
                    primitive: None,
                    owner: owner.clone(),
                    target: slot.target,
                    signature: NativeSignature {
                        params: method
                            .params
                            .iter()
                            .map(|parameter| parameter.ty.clone())
                            .collect(),
                        result: method.return_type.clone(),
                    },
                    retention: retention.clone(),
                });
            }
            // An associated result adapter already selects the output's table.
            // Native defaults can use its operations without resolving a trait.
            if let Some(view) = &table.view {
                for result in &view.results {
                    pending.push(calls::interface_binding(
                        owner,
                        &result.implementation,
                        Some(environment.clone()),
                    )?);
                }
            }
            for parent in &table.parents {
                pending.push(calls::interface_binding(
                    owner,
                    &parent.implementation,
                    Some(environment.clone()),
                )?);
            }
        }
        Ok(operations)
    }
}
