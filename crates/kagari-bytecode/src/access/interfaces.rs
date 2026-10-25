//! Interface construction contributes one checked receiver-to-interface flow.
use crate::{
    instruction::ConstructionTarget,
    module::{BytecodeFunction, BytecodeModule},
    program::ModuleRef,
};
use kagari_common::identity::table::DefinitionTable;
use kagari_contract::types::PublicItem;
use kagari_types::ty::Ty;

pub(super) struct ConstructionTypes {
    pub(super) receiver: Ty,
    pub(super) interface: Ty,
    pub(super) requires_fact: bool,
}

pub(super) fn construction_types<'a>(
    target: &ConstructionTarget,
    function: &BytecodeFunction,
    owner: &impl Fn(ModuleRef) -> Option<&'a BytecodeModule>,
    definitions: &DefinitionTable,
) -> Option<ConstructionTypes> {
    match target {
        ConstructionTarget::Bound(required) => Some(ConstructionTypes {
            receiver: required.receiver.clone(),
            interface: Ty::Trait(required.interface.clone()),
            requires_fact: true,
        }),
        ConstructionTarget::Declared {
            module,
            implementation,
            arguments,
            ..
        } => {
            let owner = owner(*module)?;
            let linked = owner.interface_tables.get(implementation.index())?;
            let table = owner.public_items.iter().find_map(|item| match item {
                PublicItem::InterfaceTable(table) if table.declaration == linked.declaration => {
                    table.instantiate(
                        arguments,
                        function
                            .metadata
                            .semantic
                            .generic
                            .as_ref()
                            .map_or(&[], |body| body.parameters.as_slice()),
                        definitions,
                    )
                }
                _ => None,
            })?;
            Some(ConstructionTypes {
                receiver: table.for_type,
                interface: table.trait_type,
                requires_fact: false,
            })
        }
    }
}
