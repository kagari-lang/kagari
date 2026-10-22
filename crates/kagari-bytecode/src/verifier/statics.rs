//! Static operands are local indices into checked supplier bindings, not raw values.
use crate::{
    instruction::{BytecodeInstruction, CallTarget, ModuleSlot, Slot},
    module::{BytecodeFunction, BytecodeModule, CallableTarget},
    program::BytecodeProgram,
    verifier::BytecodeVerificationError,
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionTable};
use kagari_contract::{representation::semantic_representation, types::PublicItem};
use kagari_types::visibility::Visibility;
use std::collections::HashSet;

pub(super) fn verify(
    module: &BytecodeModule,
    program: Option<&BytecodeProgram>,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), BytecodeVerificationError> {
    let invalid = || BytecodeVerificationError::InvalidPublicAbi;
    let mut declarations = HashSet::new();
    let mut initializers = HashSet::new();
    for slot in &module.module_slots {
        cancel
            .check()
            .map_err(|_| BytecodeVerificationError::Cancelled)?;
        let contract = &slot.contract;
        if !contract.valid(definitions, cancel)
            || !contract.accessible_from(&module.identity)
            || !declarations.insert(contract.declaration)
        {
            return Err(invalid());
        }
        let owner = if contract.supplier == module.identity {
            module
        } else {
            let program = program.ok_or_else(invalid)?;
            module
                .dependencies
                .iter()
                .filter_map(|id| program.modules.get(id.index()))
                .find(|owner| owner.identity == contract.supplier)
                .ok_or_else(invalid)?
        };
        if !owner.module_slots.iter().any(|declared| declared == slot) {
            return Err(invalid());
        }
        let initializer = owner
            .functions
            .get(slot.initializer.index())
            .ok_or_else(invalid)?;
        if initializer.identity.as_ref().is_some_and(|identity| {
            !contract.accepts_initializer_identity(identity.declaration, definitions)
        }) || !initializer.metadata.params.is_empty()
            || initializer.parameter_count != 0
            || initializer.metadata.semantic.generic.is_some()
            || initializer.metadata.semantic.protocol_adapter.is_some()
            || initializer.metadata.semantic.result.as_ref() != Some(&contract.ty)
            || initializer.metadata.return_type != semantic_representation(&contract.ty)
            || initializer.metadata.effects.may_suspend
            || initializer.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    BytecodeInstruction::MakeFuture { .. } | BytecodeInstruction::Await { .. }
                )
            })
        {
            return Err(invalid());
        }
        if owner.identity == module.identity {
            if !initializers.insert(slot.initializer) {
                return Err(invalid());
            }
            let public = module
                .public_items
                .iter()
                .filter(|item| matches!(item, PublicItem::Static(value) if value == contract))
                .count();
            if public != usize::from(contract.visibility == Visibility::Public) {
                return Err(invalid());
            }
            if module.public_items.iter().any(|item| {
                matches!(item, PublicItem::Function(value) if value.name == initializer.name
                || initializer.identity.as_ref().is_some_and(|identity| {
                    definitions.resolve(identity.declaration).is_ok_and(|view| {
                        view.last().is_some_and(|part| part.name == value.name)
                    })
                }))
            }) || initializer.identity.as_ref().is_some_and(|identity| {
                program.is_some_and(|program| {
                    program.exports.entry == Some(identity.declaration)
                        || program.exports.namespaces.values().any(|namespace| {
                            namespace
                                .functions
                                .values()
                                .any(|id| *id == identity.declaration)
                        })
                })
            }) {
                return Err(invalid());
            }
        }
    }
    for item in &module.public_items {
        if let PublicItem::Static(contract) = item
            && !module
                .module_slots
                .iter()
                .any(|slot| slot.contract == *contract && contract.supplier == module.identity)
        {
            return Err(invalid());
        }
    }
    // No executable operand grants initializer authority. Only a runtime-owned
    // return transition may use this entry; ordinary calls/closures cannot forge it.
    let hidden = |owner: &BytecodeModule, function| {
        owner
            .module_slots
            .iter()
            .any(|slot| slot.contract.supplier == owner.identity && slot.initializer == function)
    };
    let external = |id, function| {
        program
            .and_then(|program: &BytecodeProgram| program.modules.get(id))
            .is_some_and(|owner| hidden(owner, function))
    };
    for function in &module.functions {
        for instruction in &function.instructions {
            cancel
                .check()
                .map_err(|_| BytecodeVerificationError::Cancelled)?;
            let forbidden = match instruction {
                BytecodeInstruction::MakeClosure { function, .. }
                | BytecodeInstruction::MakeFuture { function, .. } => hidden(module, *function),
                BytecodeInstruction::Call {
                    callee: CallTarget::Function(function),
                    ..
                } => hidden(module, *function),
                BytecodeInstruction::Call {
                    callee: CallTarget::ModuleFunction { module, function },
                    ..
                } => external(module.index(), *function),
                BytecodeInstruction::Call {
                    callee:
                        CallTarget::Shared {
                            module,
                            target: CallableTarget::Script(function),
                            ..
                        },
                    ..
                } => external(module.index(), *function),
                _ => false,
            };
            if forbidden {
                return Err(invalid());
            }
        }
    }
    for table in &module.interface_tables {
        for method in &table.methods {
            if let CallableTarget::Script(function) = method.target
                && hidden(module, function)
            {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

pub(super) fn operand(
    module: &BytecodeModule,
    function: &BytecodeFunction,
    slot: ModuleSlot,
    value: Slot,
) -> Result<(), BytecodeVerificationError> {
    if function.metadata.semantic.slots.get(&value.index())
        != Some(&module.module_slots[slot.index()].contract.ty)
    {
        return Err(BytecodeVerificationError::InvalidOperation {
            function: function.id,
            reason: "static operand semantic type",
        });
    }
    Ok(())
}
