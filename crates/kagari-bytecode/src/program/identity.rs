//! Collect implicit declaration names after checking the complete input snapshot.
use crate::{module::BytecodeModule, verifier::BytecodeVerificationError};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        map::DefinitionContext,
        mapping::DefinitionRecord,
        table::{DefinitionTable, DefinitionTableError},
    },
};
use kagari_contract::types::PublicItem;

/// Existing references must belong to the input prefix before any names are added.
/// Collected names assign identity only; module and linked verifiers establish ABI.
pub fn collect(
    modules: &[&BytecodeModule],
    context: &DefinitionContext,
) -> Result<DefinitionTable, BytecodeVerificationError> {
    let cancel = CancellationToken::default();
    let input = context.snapshot();
    for module in modules {
        module
            .visit_definitions(&mut |id| Ok(input.resolve(*id).map(|_| ())?), &cancel)
            .map_err(|_| BytecodeVerificationError::InvalidPublicAbi)?;
    }
    for module in modules {
        collect_module(module, context).map_err(|_| BytecodeVerificationError::InvalidPublicAbi)?;
    }
    Ok(context.snapshot())
}

fn collect_module(
    module: &BytecodeModule,
    context: &DefinitionContext,
) -> Result<(), DefinitionTableError> {
    let root = context.intern_root(&module.identity)?;
    for item in &module.public_items {
        let (owner, methods) = match item {
            PublicItem::Function(function) => {
                context.intern_child(root, DefinitionKind::Function, &function.name, 0)?;
                continue;
            }
            PublicItem::Type(ty) => {
                context.intern_child(root, ty.kind.definition_kind(), &ty.name, 0)?;
                continue;
            }
            PublicItem::Trait(contract) => (
                context.intern_child(root, DefinitionKind::Trait, &contract.name, 0)?,
                &contract.methods,
            ),
            PublicItem::InterfaceTable(table) => (table.declaration, &table.methods),
            PublicItem::InherentTable(table) => (table.declaration, &table.methods),
            PublicItem::Const(_) => continue,
        };
        for method in methods {
            context.intern_child(owner, DefinitionKind::Method, &method.name, 0)?;
        }
    }
    for contract in &module.trait_contracts {
        for method in &contract.abi.methods {
            context.intern_child(
                contract.declaration,
                DefinitionKind::Method,
                &method.name,
                0,
            )?;
        }
    }
    Ok(())
}
