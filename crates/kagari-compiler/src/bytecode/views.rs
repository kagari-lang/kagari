//! Select explicit result-boxing tables from already materialized implementations.
use crate::bytecode::BytecodeLoweringError;
use kagari_abi::types::{
    self as abi, AbiType, ConcreteFunctionIdentity, InterfaceTableAbi, PublicAbiItem,
    inheritance::erased_iterator_view,
};
use kagari_bytecode::module::{BytecodeModule, InterfaceResultAdapter, InterfaceViewRecord};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
};

pub(super) fn populate(modules: &mut [BytecodeModule]) -> Result<(), BytecodeLoweringError> {
    let mut views = Vec::new();
    for (owner, module) in modules.iter().enumerate() {
        for (index, linked) in module.interface_tables.iter().enumerate() {
            let Some(table) = applied(module, &linked.declaration, &linked.arguments) else {
                continue;
            };
            if !table.for_type.is_concrete() || !table.trait_type.is_concrete() {
                continue;
            }
            let AbiType::Trait(interface) = &table.trait_type else {
                continue;
            };
            let lookup = |id: &DefinitionId| {
                let owner = modules.iter().find(|module| module.identity == id.module)?;
                abi::trait_contract(
                    &owner.identity,
                    &owner.public_items,
                    &owner.trait_contracts,
                    id,
                )
            };
            let Some(view) = erased_iterator_view(
                interface,
                &table.for_type,
                &CancellationToken::default(),
                &lookup,
            )
            .map_err(|_| BytecodeLoweringError::InvalidNativeInterface)?
            else {
                continue;
            };
            let contract =
                lookup(&view.declaration).ok_or(BytecodeLoweringError::InvalidNativeInterface)?;
            let declaration_owner = modules
                .iter()
                .find(|owner| owner.identity == view.declaration.module)
                .ok_or(BytecodeLoweringError::InvalidNativeInterface)?;
            let mut results = Vec::new();
            for (slot, method) in contract.methods.iter().enumerate() {
                let raw = table
                    .methods
                    .iter()
                    .find(|raw| raw.name == method.name)
                    .ok_or(BytecodeLoweringError::InvalidNativeInterface)?;
                let (_, result) = abi::interface_method_semantics(
                    &declaration_owner.identity,
                    &declaration_owner.public_items,
                    &declaration_owner.trait_contracts,
                    &view,
                    slot,
                )
                .ok_or(BytecodeLoweringError::InvalidNativeInterface)?;
                if raw.return_type == result {
                    continue;
                }
                let mut matches = modules.iter().flat_map(|owner| {
                    owner.interface_tables.iter().filter_map(|record| {
                        let target = applied(owner, &record.declaration, &record.arguments)?;
                        (target.for_type == raw.return_type && target.trait_type == result).then(
                            || ConcreteFunctionIdentity {
                                declaration: record.declaration.clone(),
                                arguments: record.arguments.clone(),
                            },
                        )
                    })
                });
                let implementation = matches
                    .next()
                    .ok_or(BytecodeLoweringError::InvalidNativeInterface)?;
                if matches.next().is_some() {
                    return Err(BytecodeLoweringError::InvalidNativeInterface);
                }
                let mut method_id = view.declaration.clone();
                method_id.path.push(DefinitionPathSegment {
                    kind: DefinitionKind::Method,
                    name: method.name.clone(),
                    occurrence: 0,
                });
                results.push(InterfaceResultAdapter {
                    method: method_id,
                    implementation,
                });
            }
            views.push((
                owner,
                index,
                InterfaceViewRecord {
                    interface: view,
                    results,
                },
            ));
        }
    }
    for (owner, index, view) in views {
        modules[owner].interface_tables[index].view = Some(view);
    }
    Ok(())
}

fn applied(
    module: &BytecodeModule,
    declaration: &DefinitionId,
    arguments: &[AbiType],
) -> Option<InterfaceTableAbi> {
    module.public_items.iter().find_map(|item| match item {
        PublicAbiItem::InterfaceTable(table) if table.declaration == *declaration => {
            table.instantiate(arguments)
        }
        _ => None,
    })
}
