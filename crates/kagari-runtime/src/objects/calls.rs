//! Generic calls retain selections from the supplying generation.
use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    execution_metadata::{groups::OperationId, operation::BoundOperation},
    frame::{
        ExecutionFrame,
        types::{TypeEnvironment, compatibility::TypeView, operations::OperationBindings},
    },
    gc::interfaces::InterfaceResultBinding,
    module::LoadedModule,
    native::registry::callable_primitive,
    value::Value,
};
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{
    callable::{
        interface::InterfaceCallContract,
        witness::{OperationWitness, SharedMethodWitness},
    },
    types as abi,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_types::{
    callable::CallableImplementation, declaration::requirement::NativeCallableRequirement, ty::Ty,
};

pub(super) fn interface_binding(
    implementation: &LoadedModule,
    application: &ConcreteFunctionIdentity<DefinitionId>,
    environment: Option<TypeEnvironment>,
) -> Result<InterfaceResultBinding, RuntimeError> {
    let invalid = || RuntimeError::module_validation("invalid selected interface binding");
    let module = implementation.definition(application.declaration)?.module();
    let owner = implementation
        .members()
        .find(|owner| &owner.bytecode.identity == module)
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

struct OperationScope<'a> {
    owner: &'a LoadedModule,
    environment: Option<TypeEnvironment>,
}

impl OperationScope<'_> {
    fn resolve(&self, ty: &Ty<DefinitionId>) -> Result<Ty<DefinitionId>, RuntimeError> {
        if ty.is_concrete() {
            return Ok(ty.clone());
        }
        self.environment
            .as_ref()
            .ok_or_else(|| RuntimeError::module_validation("missing generic call environment"))?
            .types
            .resolve(ty)
    }
}

fn resolve_requirement(
    scope: &OperationScope<'_>,
    required: &NativeCallableRequirement<DefinitionId>,
) -> Result<NativeCallableRequirement<DefinitionId>, RuntimeError> {
    let Ty::Trait(interface) = scope.resolve(&Ty::Trait(required.interface.clone()))? else {
        return Err(RuntimeError::module_validation("constraint interface type"));
    };
    Ok(NativeCallableRequirement {
        receiver: scope.resolve(&required.receiver)?,
        interface,
        member: required.member,
        arguments: required
            .arguments
            .iter()
            .map(|ty| scope.resolve(ty))
            .collect::<Result<_, _>>()?,
    })
}

impl Runtime {
    pub fn resolve_interface_call(
        &self,
        frame: &ExecutionFrame,
        contract: &InterfaceCallContract<DefinitionId>,
        receiver: &Value,
    ) -> Result<RootedInterfaceMethod, RuntimeError> {
        let invalid = || RuntimeError::module_validation("generic call operation environment");
        let Ty::Trait(interface) = frame
            .resolve_type(&Ty::Trait(contract.interface.clone()))?
            .into_owned()
        else {
            return Err(invalid());
        };
        if let Some(ty) = &contract.receiver {
            let receiver_type = frame.resolve_type(ty)?;
            let environment = frame.environment().ok_or_else(invalid)?;
            let operation = environment
                .operation_slot(&self.gc, &receiver_type, &interface, contract.method_slot)
                .ok_or_else(invalid)?;
            let method = RootedInterfaceMethod::from_operation(
                self,
                self.root_value(*receiver).ok_or_else(invalid)?,
                operation,
                *receiver,
                receiver_type.into_owned(),
                interface,
            )?;
            let arguments = self.type_arguments(
                frame.loaded(),
                frame
                    .environment()
                    .map(|environment| environment.types.clone()),
                &contract.arguments,
            )?;
            let method = self.apply_interface_method(
                method,
                &arguments,
                self.bind_operations(frame, &contract.operations)?,
            )?;
            return Ok(method);
        }
        let arguments = self.type_arguments(
            frame.loaded(),
            frame
                .environment()
                .map(|environment| environment.types.clone()),
            &contract.arguments,
        )?;
        let method = self.prepare_interface_method_slot(
            receiver,
            &interface,
            contract.method_slot as usize,
            &arguments,
            self.bind_operations(frame, &contract.operations)?,
        )?;
        let compatible = {
            let view = method.view(self)?;
            TypeView::new(
                &Ty::Trait(method.interface_expression().clone()),
                view.implementation(),
                view.receiver_environment()
                    .map(|environment| environment.types.as_ref()),
            )
            .compatible(TypeView::new(
                &Ty::Trait(contract.interface.clone()),
                frame.loaded(),
                frame
                    .environment()
                    .as_ref()
                    .map(|environment| environment.types.as_ref()),
            ))
        };
        if !compatible {
            return Err(invalid());
        }
        Ok(method)
    }

    pub(crate) fn bind_operations(
        &self,
        frame: &ExecutionFrame,
        witnesses: &[OperationWitness<DefinitionId>],
    ) -> Result<OperationBindings, RuntimeError> {
        self.bind_operations_in(frame.loaded(), frame.environment(), witnesses)
    }

    /// Bind already-verified witnesses in their lexical executable scope. Host
    /// bindings use this same path without manufacturing an execution frame.
    pub(crate) fn bind_operations_in(
        &self,
        owner: &LoadedModule,
        environment: Option<TypeEnvironment>,
        witnesses: &[OperationWitness<DefinitionId>],
    ) -> Result<OperationBindings, RuntimeError> {
        let scope = OperationScope { owner, environment };
        let invalid = || RuntimeError::module_validation("generic call operation environment");
        let mut operations = OperationBindings::default();
        for witness in witnesses {
            let operation = match witness {
                OperationWitness::SharedMethod(selected) => {
                    self.bind_shared_method(&scope, selected)?
                }
                OperationWitness::Forward(required) => {
                    let required = resolve_requirement(&scope, required)?;
                    scope
                        .environment
                        .as_ref()
                        .and_then(|environment| environment.operation(&self.gc, &required))
                        .ok_or_else(invalid)?
                }
                OperationWitness::Selected(selected) => {
                    let module = scope
                        .owner
                        .definition(selected.instance.declaration)?
                        .module();
                    let owner = scope
                        .owner
                        .members()
                        .find(|owner| &owner.bytecode.identity == module)
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
                    let module = scope
                        .owner
                        .definition(selected.requirement.interface.declaration)?
                        .module();
                    let slot = owner
                        .members()
                        .find(|member| &member.bytecode.identity == module)
                        .and_then(|member| {
                            abi::trait_contract_in(
                                Some(member.definitions()),
                                &member.bytecode.identity,
                                &member.bytecode.public_items,
                                &member.bytecode.trait_contracts,
                                &selected.requirement.interface.declaration,
                            )
                            .and_then(|contract| {
                                contract.methods.iter().position(|method| {
                                    member.definition_name(selected.requirement.member)
                                        == Some(method.name.as_str())
                                })
                            })
                        })
                        .ok_or_else(invalid)? as u32;
                    // The callable is already selected and proved. Read outputs
                    // from its pinned implementation table; never search for a
                    // different implementation while resolving a projection.
                    let mut associated_interface = selected.requirement.interface.clone();
                    for table in &owner.bytecode.interface_tables {
                        if !table.methods.iter().any(|method| {
                            method.method == selected.requirement.member && method.target == target
                        }) {
                            continue;
                        }
                        let Some(template) =
                            owner
                                .bytecode
                                .public_items
                                .iter()
                                .find_map(|item| match item {
                                    PublicItem::InterfaceTable(template)
                                        if template.declaration == table.declaration =>
                                    {
                                        Some(template)
                                    }
                                    _ => None,
                                })
                        else {
                            continue;
                        };
                        let Some(applied) = template.instantiate_scoped(
                            &table.arguments,
                            &[],
                            Some(owner.definitions()),
                        ) else {
                            continue;
                        };
                        let Ty::Trait(interface) = applied.trait_type else {
                            continue;
                        };
                        if applied.for_type == selected.requirement.receiver
                            && interface.declaration == associated_interface.declaration
                            && interface.arguments == associated_interface.arguments
                            && selected
                                .requirement
                                .interface
                                .associated_types
                                .iter()
                                .all(|(id, ty)| interface.associated_types.get(id) == Some(ty))
                        {
                            associated_interface = interface;
                        }
                    }
                    self.gc.alloc_bound_operation(BoundOperation {
                        associated_interface,
                        generic: None,
                        slot,
                        primitive: callable_primitive(selected, owner.definitions()),
                        requirement: selected.requirement.clone(),
                        owner,
                        target,
                        signature: selected.signature.clone(),
                    })?
                }
            };
            operations.push(&self.gc, operation)?;
        }
        Ok(operations)
    }
}

impl Runtime {
    fn bind_shared_method(
        &self,
        scope: &OperationScope<'_>,
        selected: &SharedMethodWitness<DefinitionId>,
    ) -> Result<OperationId, RuntimeError> {
        let binding = interface_binding(
            scope.owner,
            &selected.implementation,
            scope.environment.clone(),
        )?;
        let required = resolve_requirement(scope, &selected.requirement)?;
        let group = self.bind_table_operations(&binding)?;
        self.gc
            .operation_group(group)
            .and_then(|group| group.operation(&required))
            .ok_or_else(|| RuntimeError::module_validation("shared constraint method selection"))
    }
}
