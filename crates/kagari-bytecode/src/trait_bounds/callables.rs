//! Selected native callbacks must have a matching concrete executable target.
use crate::module::BytecodeModule;
use kagari_common::identity::table::DefinitionTable;
use kagari_contract::{
    callable::witness::OperationWitness,
    native_import::callables::{NativeCallableApplication, NativeCallableOrigin},
    representation::semantic_representation,
};
use kagari_types::callable::CallableImplementation;

pub(super) fn witness_valid(
    operation: &OperationWitness,
    closure: &[&BytecodeModule],
    definitions: &DefinitionTable,
) -> bool {
    operation
        .walk()
        .all(|operation| witness_target_valid(operation, closure, definitions))
}

fn witness_target_valid(
    operation: &OperationWitness,
    closure: &[&BytecodeModule],
    definitions: &DefinitionTable,
) -> bool {
    match operation {
        OperationWitness::SharedAdapter(adapter) => {
            let Some((body, required, signature)) = adapter.template(definitions) else {
                return false;
            };
            closure
                .iter()
                .flat_map(|owner| &owner.functions)
                .find(|function| function.identity.as_ref() == Some(&adapter.call.instance))
                .is_some_and(|function| {
                    let semantic = &function.metadata.semantic;
                    semantic.generic.as_ref() == Some(&body)
                        && semantic.protocol_adapter.as_ref() == Some(&required)
                        && semantic.params.len() == signature.params.len()
                        && signature
                            .params
                            .iter()
                            .enumerate()
                            .all(|(index, ty)| semantic.params.get(&index) == Some(ty))
                        && semantic.result.as_ref() == Some(&signature.result)
                })
        }
        OperationWitness::Forward(_) => true,
        OperationWitness::Selected(call) => target_valid(call, closure, definitions),
        OperationWitness::SharedMethod(selected) => closure
            .iter()
            .find(|owner| {
                definitions
                    .resolve(selected.implementation.declaration)
                    .is_ok_and(|view| view.module() == &owner.identity)
            })
            .is_some_and(|owner| {
                owner.interface_tables.iter().any(|table| {
                    table.declaration == selected.implementation.declaration
                        && if selected
                            .implementation
                            .arguments
                            .iter()
                            .any(|ty| !ty.is_concrete())
                        {
                            table.arguments.iter().any(|ty| !ty.is_concrete())
                        } else {
                            table.arguments == selected.implementation.arguments
                        }
                        && table
                            .methods
                            .iter()
                            .any(|slot| slot.method == selected.requirement.member)
                })
            }),
    }
}

pub(super) fn target_valid(
    call: &NativeCallableApplication,
    closure: &[&BytecodeModule],
    definitions: &DefinitionTable,
) -> bool {
    let Some(owner) = closure.iter().find(|owner| {
        definitions
            .resolve(call.instance.declaration)
            .is_ok_and(|view| view.module() == &owner.identity)
    }) else {
        return false;
    };
    match &call.implementation {
        CallableImplementation::Script => owner
            .functions
            .iter()
            .find(|function| function.identity.as_ref() == Some(&call.instance))
            .is_some_and(|function| {
                (match call.origin {
                    NativeCallableOrigin::Implementation => {
                        function.metadata.semantic.protocol_adapter.is_none()
                    }
                    NativeCallableOrigin::ProtocolAdapter => {
                        function.metadata.semantic.protocol_adapter.as_ref()
                            == Some(&call.requirement)
                    }
                }) && function.metadata.params.len() == call.signature.params.len()
                    && function
                        .metadata
                        .params
                        .iter()
                        .zip(&call.signature.params)
                        .all(|(physical, semantic)| *physical == semantic_representation(semantic))
                    && function.metadata.return_type
                        == semantic_representation(&call.signature.result)
                    && function.metadata.semantic.params.len() == call.signature.params.len()
                    && call.signature.params.iter().enumerate().all(|(index, ty)| {
                        function.metadata.semantic.params.get(&index) == Some(ty)
                    })
                    && function.metadata.semantic.result.as_ref() == Some(&call.signature.result)
            }),
        CallableImplementation::Native(binding) => owner.native_imports.iter().any(|import| {
            import.instance == call.instance
                && &import.binding == binding
                && import.signature == call.signature
                && import.host.is_none()
        }),
        CallableImplementation::Required | CallableImplementation::NativeDefault(_) => false,
    }
}
