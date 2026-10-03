//! Generic calls retain selections from the supplying generation.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    frame::{
        ExecutionFrame,
        types::{BoundOperation, TypeEnvironment, compatibility::TypeView},
    },
    gc::interfaces::InterfaceResultBinding,
    module::LoadedModule,
    native::registry::callable_primitive,
    value::Value,
};
use kagari_abi::{
    callable::{
        CallableImplementation,
        interface::InterfaceCallContract,
        witness::{OperationWitness, SharedMethodWitness},
    },
    native_import::callables::NativeCallableRequirement,
    types::{self as abi, AbiType, ConcreteFunctionIdentity},
};
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use std::rc::Rc;

pub(super) fn interface_binding(
    implementation: &LoadedModule,
    application: &ConcreteFunctionIdentity,
    environment: Option<Rc<TypeEnvironment>>,
) -> Result<InterfaceResultBinding, RuntimeError> {
    let invalid = || RuntimeError::module_validation("invalid selected interface binding");
    let owner = implementation
        .members()
        .find(|owner| owner.bytecode.identity == application.declaration.module)
        .ok_or_else(invalid)?;
    let shared = application.arguments.iter().any(|ty| !ty.is_concrete());
    let table = owner
        .bytecode
        .interface_tables
        .iter()
        .position(|table| {
            table.declaration == application.declaration
                && if shared {
                    table.arguments.iter().any(|ty| !ty.is_concrete())
                } else {
                    table.arguments == application.arguments
                }
        })
        .ok_or_else(invalid)?;
    Ok(InterfaceResultBinding {
        owner,
        table,
        arguments: application.arguments.clone(),
        environment,
    })
}

fn resolve_requirement(
    frame: &ExecutionFrame,
    required: &NativeCallableRequirement,
) -> Result<NativeCallableRequirement, RuntimeError> {
    let AbiType::Trait(interface) = frame
        .resolve_type(&AbiType::Trait(required.interface.clone()))?
        .into_owned()
    else {
        return Err(RuntimeError::module_validation("constraint interface type"));
    };
    Ok(NativeCallableRequirement {
        receiver: frame.resolve_type(&required.receiver)?.into_owned(),
        interface,
        member: required.member.clone(),
        arguments: required
            .arguments
            .iter()
            .map(|ty| frame.resolve_type(ty).map(|ty| ty.into_owned()))
            .collect::<Result<_, _>>()?,
    })
}

impl Runtime {
    pub fn resolve_interface_call(
        &self,
        frame: &ExecutionFrame,
        contract: &InterfaceCallContract,
        receiver: &Value,
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        let invalid = || RuntimeError::module_validation("generic call operation environment");
        let AbiType::Trait(interface) = frame
            .resolve_type(&AbiType::Trait(contract.interface.clone()))?
            .into_owned()
        else {
            return Err(invalid());
        };
        if let Some(ty) = &contract.receiver {
            let receiver_type = frame.resolve_type(ty)?;
            let environment = frame.environment().ok_or_else(invalid)?;
            let operation = environment
                .operation_slot(&receiver_type, &interface, contract.method_slot)
                .ok_or_else(invalid)?;
            self.validate_loaded_module(&operation.owner)?;
            let method = RootedInterfaceMethod::from_operation(
                self.root_value(receiver.clone()).ok_or_else(invalid)?,
                operation.clone(),
                receiver.clone(),
                receiver_type.into_owned(),
                interface,
            );
            let arguments =
                self.type_arguments(frame.loaded(), frame.environment(), &contract.arguments)?;
            let mut method = self.apply_interface_method(method, &arguments)?;
            if let Some(environment) = &mut method.environment {
                Rc::make_mut(environment)
                    .operations
                    .extend(self.bind_operations(frame, &contract.operations)?);
            }
            return Ok(method);
        }
        let arguments =
            self.type_arguments(frame.loaded(), frame.environment(), &contract.arguments)?;
        let mut method = self.resolve_interface_method_slot(
            receiver,
            &interface,
            contract.method_slot as usize,
            &arguments,
        )?;
        if !TypeView::new(
            &AbiType::Trait(method.interface_expression().clone()),
            method.implementation(),
            method.receiver_environment().map(Rc::as_ref),
        )
        .compatible(TypeView::new(
            &AbiType::Trait(contract.interface.clone()),
            frame.loaded(),
            frame.environment().as_deref(),
        )) {
            return Err(invalid());
        }
        if let Some(environment) = &mut method.environment {
            Rc::make_mut(environment)
                .operations
                .extend(self.bind_operations(frame, &contract.operations)?);
        }
        Ok(method)
    }

    pub(crate) fn bind_operations(
        &self,
        frame: &ExecutionFrame,
        witnesses: &[OperationWitness],
    ) -> Result<Vec<Rc<BoundOperation>>, RuntimeError> {
        let invalid = || RuntimeError::module_validation("generic call operation environment");
        let mut operations = Vec::with_capacity(witnesses.len());
        let mut supplying_program = None;
        for witness in witnesses {
            let operation = match witness {
                OperationWitness::SharedMethod(selected) => {
                    self.bind_shared_method(frame, selected)?
                }
                OperationWitness::Forward(required) => {
                    let required = resolve_requirement(frame, required)?;
                    frame
                        .environment()
                        .and_then(|environment| environment.operation(&required).cloned())
                        .ok_or_else(invalid)?
                }
                OperationWitness::Selected(selected) => {
                    let retention = match &supplying_program {
                        Some(retention) => Rc::clone(retention),
                        None => {
                            let retention = Rc::new(
                                self.modules
                                    .retain_runtime_program(frame.loaded())
                                    .ok_or_else(invalid)?,
                            );
                            supplying_program = Some(retention.clone());
                            retention
                        }
                    };
                    let owner = frame
                        .loaded()
                        .members()
                        .find(|owner| {
                            owner.bytecode.identity == selected.instance.declaration.module
                        })
                        .ok_or_else(invalid)?;
                    let target = match &selected.implementation {
                        CallableImplementation::Script => owner
                            .bytecode
                            .functions
                            .iter()
                            .find(|function| function.identity.as_ref() == Some(&selected.instance))
                            .map(|function| CallableTarget::Script(function.id)),
                        CallableImplementation::Native(binding) => owner
                            .bytecode
                            .native_imports
                            .iter()
                            .position(|import| {
                                import.instance == selected.instance
                                    && import.binding == *binding
                                    && import.signature == selected.signature
                            })
                            .map(|index| CallableTarget::Native(NativeImportId::new(index))),
                        _ => None,
                    }
                    .ok_or_else(invalid)?;
                    let slot = owner
                        .members()
                        .find(|member| {
                            member.bytecode.identity
                                == selected.requirement.interface.declaration.module
                        })
                        .and_then(|member| {
                            abi::trait_contract(
                                &member.bytecode.identity,
                                &member.bytecode.public_items,
                                &member.bytecode.trait_contracts,
                                &selected.requirement.interface.declaration,
                            )
                            .and_then(|contract| {
                                contract.methods.iter().position(|method| {
                                    selected
                                        .requirement
                                        .member
                                        .path
                                        .last()
                                        .is_some_and(|segment| segment.name == method.name)
                                })
                            })
                        })
                        .ok_or_else(invalid)? as u32;
                    Rc::new(BoundOperation {
                        generic: None,
                        retention,
                        slot,
                        primitive: callable_primitive(selected),
                        requirement: selected.requirement.clone(),
                        owner,
                        target,
                        signature: selected.signature.clone(),
                    })
                }
            };
            operations.push(operation);
        }
        Ok(operations)
    }
}

impl Runtime {
    fn bind_shared_method(
        &self,
        frame: &ExecutionFrame,
        selected: &SharedMethodWitness,
    ) -> Result<Rc<BoundOperation>, RuntimeError> {
        let binding = interface_binding(
            frame.loaded(),
            &selected.implementation,
            frame.environment(),
        )?;
        let required = resolve_requirement(frame, &selected.requirement)?;
        self.bind_table_operations(&binding)?
            .into_iter()
            .find(|operation| operation.requirement == required)
            .ok_or_else(|| RuntimeError::module_validation("shared constraint method selection"))
    }
}
