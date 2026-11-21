//! Decide which linked declarations can be inputs to authoring, never executable providers.
use kagari_bytecode::module::BytecodeModule;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        mapping::DefinitionMappingError,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::types::{InterfaceTable, PublicItem};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, TypeDefKind},
    language::{Protocol, requirements, schema::constructor_shape_valid},
};
use std::collections::HashSet;

fn script_signature(method: &FnDecl) -> bool {
    method.host_policy.is_none() && method.method_policy.receiver_access.is_none()
}

/// Select public nominal declarations that carry no native capability.
pub(super) fn nominal(
    item: &PublicItem,
    root: DefinitionId,
    defs: &DefinitionTable,
) -> Option<DefinitionId> {
    match item {
        PublicItem::Type(ty) => {
            let id = defs.lookup_child(root, ty.kind.definition_kind(), &ty.name, 0)?;
            let allowed = match ty.kind {
                TypeDefKind::Struct | TypeDefKind::Enum => {
                    ty.fields.iter().all(|f| f.host_policy.is_none())
                }
                TypeDefKind::Native(_) => constructor_shape_valid(id, ty, defs),
                _ => false,
            };
            allowed.then_some(id)
        }
        PublicItem::Trait(tr) => {
            let id = defs.lookup_child(root, DefinitionKind::Trait, &tr.name, 0)?;
            let relation = if tr.name == "TryFrom"
                && defs.lookup_root(&Protocol::From.module()) == Some(root)
            {
                requirements::trait_shape_valid(id, tr, defs)
            } else {
                tr.conversion_adapter.is_none()
            };
            (relation
                && tr.methods.iter().all(|m| {
                    script_signature(m)
                        && matches!(
                            m.implementation,
                            CallableImplementation::Required | CallableImplementation::Script
                        )
                }))
            .then_some(id)
        }
        _ => None,
    }
}

pub(super) fn public_nominals(
    modules: &[BytecodeModule],
    defs: &DefinitionTable,
) -> HashSet<DefinitionId> {
    let mut public = HashSet::new();
    for module in modules {
        if let Some(root) = defs.lookup_root(&module.identity) {
            public.extend(
                module
                    .public_items
                    .iter()
                    .filter_map(|item| nominal(item, root, defs)),
            );
        }
    }
    public
}

/// Linked verification pairs each non-bridge table with its authored impl header. Some
/// tables serve private traits/receivers; their presence in public_items is not visibility.
pub(super) fn source_impl(
    table: &InterfaceTable,
    public: &HashSet<DefinitionId>,
    defs: &DefinitionTable,
) -> Result<bool, DefinitionMappingError> {
    if table.bridge.is_some()
        || !table
            .methods
            .iter()
            .all(|m| script_signature(m) && m.implementation == CallableImplementation::Script)
    {
        return Ok(false);
    }
    let mut visible = true;
    table.visit_definition_ids(
        &mut |id| {
            let mut owner = *id;
            while let Some(parent) = defs.parent(owner)? {
                if defs.parent(parent)?.is_none() {
                    break;
                }
                owner = parent;
            }
            visible &= owner == table.declaration || public.contains(&owner);
            Ok(())
        },
        &CancellationToken::default(),
    )?;
    Ok(visible)
}
