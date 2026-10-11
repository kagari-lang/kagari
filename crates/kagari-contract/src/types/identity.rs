//! ABI-owned names shared by executable registration boundaries.
use crate::types::{PublicItem, TraitContract};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, map::DefinitionContext, mapping::DefinitionMappingError,
        table::DefinitionId,
    },
};

/// Register implicit ABI names without granting declaration or execution authority.
///
/// Callers must validate all input references against their original context prefix
/// before collecting names for any module in a batch. This operation appends module,
/// public item and method identities; it does not change existing IDs. A marker trait
/// with no identity-bearing members still receives its named declaration identity.
///
/// Cancellation and identity allocation failures return an error. Appended names
/// may remain after failure, but callers must not publish unverified records.
pub fn collect(
    module: &ModuleIdentity,
    items: &[PublicItem<DefinitionId>],
    contracts: &[TraitContract<DefinitionId>],
    context: &DefinitionContext,
    cancel: &CancellationToken,
) -> Result<(), DefinitionMappingError> {
    cancel
        .check()
        .map_err(|_| DefinitionMappingError::Cancelled)?;
    let root = context.intern_root(module)?;
    for item in items {
        cancel
            .check()
            .map_err(|_| DefinitionMappingError::Cancelled)?;
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
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            context.intern_child(owner, DefinitionKind::Method, &method.name, 0)?;
        }
    }
    for contract in contracts {
        for method in &contract.abi.methods {
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
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
