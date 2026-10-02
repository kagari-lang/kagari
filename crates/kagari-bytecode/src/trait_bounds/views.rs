//! Verify representation adapters without changing the canonical implementation contract.
use crate::{
    module::{BytecodeModule, InterfaceTableRecord},
    trait_bounds::contract,
};
use kagari_abi::types::{
    self as abi, AbiType, InterfaceTableAbi, PublicAbiItem, inheritance::erased_iterator_view,
    substitution::TypeTransformError,
};
use kagari_common::cancellation::CancellationToken;

pub(super) fn valid(
    module: &BytecodeModule,
    closure: &[&BytecodeModule],
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    for linked in &module.interface_tables {
        let Some(table) = applied(module, linked) else {
            if linked.view.is_some() {
                return Ok(false);
            }
            continue;
        };
        if !table.for_type.is_concrete() || !table.trait_type.is_concrete() {
            if linked.view.is_some() {
                return Ok(false);
            }
            continue;
        }
        let AbiType::Trait(interface) = &table.trait_type else {
            return Ok(false);
        };
        let expected = erased_iterator_view(interface, &table.for_type, cancel, &|id| {
            contract(id, closure)
        })?;
        let Some(view) = &linked.view else {
            if expected.is_some() {
                return Ok(false);
            }
            continue;
        };
        if expected.as_ref() != Some(&view.interface) {
            return Ok(false);
        }
        let Some(owner) = closure
            .iter()
            .find(|owner| owner.identity == view.interface.declaration.module)
        else {
            return Ok(false);
        };
        let Some(contract) = contract(&view.interface.declaration, closure) else {
            return Ok(false);
        };
        let mut count = 0;
        for (slot, declared) in contract.methods.iter().enumerate() {
            let Some(raw) = table
                .methods
                .iter()
                .find(|method| method.name == declared.name)
            else {
                return Ok(false);
            };
            let Some((params, result)) = abi::interface_method_semantics(
                &owner.identity,
                &owner.public_items,
                &owner.trait_contracts,
                &view.interface,
                slot,
            ) else {
                return Ok(false);
            };
            // Hiding outputs cannot change any input contract or expose Self.
            if params.len() != raw.params.len()
                || params
                    .iter()
                    .skip(1)
                    .zip(raw.params.iter().skip(1))
                    .any(|(a, b)| a != &b.ty)
            {
                return Ok(false);
            }
            if result == raw.return_type {
                continue;
            }
            count += 1;
            let Some(method) = linked.methods.iter().find(|slot| {
                slot.method
                    .path
                    .last()
                    .is_some_and(|part| part.name == declared.name)
            }) else {
                return Ok(false);
            };
            let mut candidates = view
                .results
                .iter()
                .filter(|adapter| adapter.method == method.method);
            let Some(adapter) = candidates.next() else {
                return Ok(false);
            };
            if candidates.next().is_some() || !matches!(&result, AbiType::Trait(_)) {
                return Ok(false);
            }
            let Some(target_owner) = closure
                .iter()
                .find(|owner| owner.identity == adapter.implementation.declaration.module)
            else {
                return Ok(false);
            };
            let Some(target) = target_owner
                .interface_tables
                .iter()
                .find(|target| {
                    target.declaration == adapter.implementation.declaration
                        && target.arguments == adapter.implementation.arguments
                })
                .and_then(|target| applied(target_owner, target))
            else {
                return Ok(false);
            };
            if target.for_type != raw.return_type || target.trait_type != result {
                return Ok(false);
            }
        }
        if count != view.results.len() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn applied(module: &BytecodeModule, linked: &InterfaceTableRecord) -> Option<InterfaceTableAbi> {
    module.public_items.iter().find_map(|item| match item {
        PublicAbiItem::InterfaceTable(table) if table.declaration == linked.declaration => {
            table.instantiate(&linked.arguments)
        }
        _ => None,
    })
}
