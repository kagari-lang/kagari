//! Generic calls retain selections from the supplying generation.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{
        call_contracts::InterfaceCallSite, groups::OperationId, operation::BoundOperation,
    },
    frame::{
        ExecutionFrame,
        types::{TypeEnvironment, compatibility::TypeView, operations::OperationBindings},
    },
    gc::interfaces::InterfaceResultBinding,
    module::LoadedModule,
    native::registry::callable_primitive,
    objects::{invocation::MethodInvocation, method_view::SelectionView},
    value::Value,
};
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{
    callable::witness::{OperationWitness, SharedMethodWitness},
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
    pub(crate) fn resolve_interface_invocation(
        &self,
        frame: &ExecutionFrame,
        site: InterfaceCallSite,
        receiver: &Value,
    ) -> Result<(MethodInvocation, Value), RuntimeError> {
        let invalid = || RuntimeError::module_validation("generic call operation environment");
        let call = self.prepare_interface_call(frame.loaded(), frame.environment(), site)?;
        if let Some(operation) = call.operation {
            let method = MethodInvocation::from_operation(self, operation)?;
            let method =
                self.apply_method_invocation(method, &call.arguments, call.operations.clone())?;
            return Ok((method, *receiver));
        }
        let id = self.select_interface_snapshot(receiver, call.interface_type())?;
        let method = MethodInvocation::from_interface(self, id, call.slot)?;
        let receiver = self.gc.interface_metadata(id).ok_or_else(invalid)?.data;
        let method =
            self.apply_method_invocation(method, &call.arguments, call.operations.clone())?;
        let compatible = {
            let view = method.view(self)?;
            let SelectionView::Interface { snapshot, .. } = &view.selection else {
                return Err(invalid());
            };
            TypeView::new(
                &Ty::Trait(snapshot.interface_expression.clone()),
                view.implementation(),
                view.receiver_environment()
                    .map(|environment| environment.types.as_ref()),
            )
            .compatible(call.interface.view(frame.loaded()))
        };
        if !compatible {
            return Err(invalid());
        }
        Ok((method, receiver))
    }

    /// Bind already-verified witnesses in their lexical executable scope. Host
    /// bindings use this same path without manufacturing an execution frame.
    pub(crate) fn bind_operations_in(
        &self,
        owner: &LoadedModule,
        environment: Option<TypeEnvironment>,
        witnesses: &[OperationWitness<DefinitionId>],
    ) -> Result<OperationBindings, RuntimeError> {
        if witnesses.is_empty() {
            return Ok(OperationBindings::default());
        }
        self.validate_loaded_module(owner)?;
        let environment_id = environment.as_ref().map(|environment| environment.id);
        if let Some(id) = environment_id
            && self.gc.environment(id).is_none()
        {
            return Err(RuntimeError::module_validation(
                "invalid operation preparation scope",
            ));
        }
        if let Some(prepared) = self
            .modules
            .operation_bindings(owner, environment_id, witnesses)
        {
            return Ok(prepared);
        }
        #[cfg(feature = "execution-diagnostics")]
        diagnostics::record(Event::OperationPreparation);
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
        self.publish_operation_bindings(owner, environment_id, witnesses, operations.clone())?;
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
