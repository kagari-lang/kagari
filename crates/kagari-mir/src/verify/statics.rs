//! Verify local and imported static bindings before sealing executable MIR.
use crate::{
    function::MirModule,
    instruction::{CallTarget, Instruction},
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionTable};
use kagari_contract::{
    exports::ProgramExports, representation::semantic_representation, types::PublicItem,
};
use kagari_types::visibility::Visibility;
use std::{collections::HashSet, slice};

pub(crate) fn valid(
    modules: &[MirModule],
    exports: &ProgramExports,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> bool {
    modules
        .iter()
        .all(|module| valid_module(module, Some((modules, exports)), definitions, cancel))
}

/// Standalone module verification checks owned entries and local authority. Imported
/// supplier bodies are checked when the enclosing program is linked, like other calls.
pub(crate) fn valid_module(
    module: &MirModule,
    program: Option<(&[MirModule], &ProgramExports)>,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> bool {
    let modules = program.map_or_else(|| slice::from_ref(module), |(modules, _)| modules);
    let mut seen = HashSet::new();
    let mut initializers = HashSet::new();
    for descriptor in &module.module_slots {
        let contract = &descriptor.contract;
        if cancel.check().is_err()
            || !contract.valid(definitions, cancel)
            || !contract.accessible_from(&module.identity)
            || !seen.insert(contract.declaration)
        {
            return false;
        }
        if contract.supplier != module.identity && !module.dependencies.contains(&contract.supplier)
        {
            return false;
        }
        let Some(owner) = modules
            .iter()
            .find(|owner| owner.identity == contract.supplier)
        else {
            if program.is_none() {
                continue;
            }
            return false;
        };
        if !owner.module_slots.contains(descriptor) {
            return false;
        }
        let Some(initializer) = owner.functions.get(descriptor.initializer.index()) else {
            return false;
        };
        if !contract.accepts_initializer_identity(initializer.instance.declaration, definitions)
            || !initializer.params.is_empty()
            || initializer.effects.may_suspend
            || initializer.semantic.generic.is_some()
            || initializer.semantic.protocol_adapter.is_some()
            || initializer.semantic.result.as_ref() != Some(&contract.ty)
            || initializer.return_type != semantic_representation(&contract.ty)
            || initializer
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| {
                    matches!(
                        instruction,
                        Instruction::MakeFuture { .. } | Instruction::Await { .. }
                    )
                })
        {
            return false;
        }
        if owner.identity == module.identity {
            if !initializers.insert(descriptor.initializer) {
                return false;
            }
            let public = module
                .abi
                .public_items
                .iter()
                .filter(|item| matches!(item, PublicItem::Static(value) if value == contract))
                .count();
            if public != usize::from(contract.visibility == Visibility::Public) {
                return false;
            }
            let id = initializer.instance.declaration;
            if program.is_some_and(|(_, exports)| {
                exports.entry == Some(id)
                    || exports
                        .namespaces
                        .values()
                        .any(|namespace| namespace.functions.values().any(|entry| *entry == id))
            }) || module.abi.public_items.iter().any(|item| {
                matches!(item, PublicItem::Function(value) if value.name == initializer.name
                || definitions.resolve(id).is_ok_and(|view| {
                    view.last().is_some_and(|part| part.name == value.name)
                }))
            }) {
                return false;
            }
        }
    }
    for item in &module.abi.public_items {
        if let PublicItem::Static(contract) = item
            && !module
                .module_slots
                .iter()
                .any(|slot| slot.contract == *contract && contract.supplier == module.identity)
        {
            return false;
        }
    }
    for function in &module.functions {
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            if cancel.check().is_err() {
                return false;
            }
            let target = match instruction {
                Instruction::Call {
                    callee: CallTarget::Function(function),
                    ..
                }
                | Instruction::MakeClosure { function, .. }
                | Instruction::MakeFuture { function, .. } => {
                    module.functions.get(function.index())
                }
                Instruction::Call {
                    callee: CallTarget::SourceFunction(contract),
                    ..
                } => modules
                    .iter()
                    .flat_map(|module| &module.functions)
                    .find(|function| {
                        function.instance.declaration == contract.declaration
                            && function.instance.arguments == contract.arguments
                    }),
                Instruction::Call {
                    callee: CallTarget::Shared(contract),
                    ..
                } => modules
                    .iter()
                    .flat_map(|module| &module.functions)
                    .find(|function| function.instance == contract.instance),
                _ => None,
            };
            if let Some(target) = target
                && modules.iter().any(|owner| {
                    owner.module_slots.iter().any(|slot| {
                        slot.contract.supplier == owner.identity
                            && owner
                                .functions
                                .get(slot.initializer.index())
                                .is_some_and(|initializer| initializer.instance == target.instance)
                    })
                })
            {
                return false;
            }
        }
    }
    true
}
