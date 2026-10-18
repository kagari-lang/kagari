//! Complete semantic collection before native preparation. Existing semantic
//! validators remain authoritative; no temporary bindings stand in for bodies.
use crate::native::{
    assembly::{
        DeclarationSnapshot, conflict,
        draft::{DraftState, defaults::lower_defaults, traits::lower_override_facts},
    },
    binding::NativeResult,
    catalog::DeclarationCatalog,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata,
        table::DefinitionTable,
    },
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{TypeDefKind, module::ModuleDecl},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    mem,
};

#[cfg(test)]
mod owner_tests;

#[cfg(test)]
mod documentation_tests;

#[cfg(test)]
mod validation_tests;

pub(super) fn seal(
    context: &DefinitionContext,
    providers: &DeclarationCatalog,
    drafts: &mut [DraftState],
) -> NativeResult<DeclarationSnapshot> {
    // Collect every trait before lowering defaults. This is not a public provider
    // snapshot: defaults and implementations may still be unexpanded intents.
    let mut contracts =
        DeclarationCatalog::declared(drafts.iter().map(|draft| &draft.declaration), context)?;
    contracts.merge(providers)?;
    for draft in drafts.iter() {
        for reference in &draft.references {
            reference.validate(&contracts)?;
        }
        for member in &draft.members {
            member.validate(&contracts)?;
        }
    }
    for draft in drafts.iter_mut() {
        lower_defaults(context, draft)?;
    }
    // Implementation expansion must see the lowered default applications, not the
    // required-method placeholders used during authoring.
    let mut contracts =
        DeclarationCatalog::declared(drafts.iter().map(|draft| &draft.declaration), context)?;
    contracts.merge(providers)?;
    for draft in drafts.iter_mut() {
        let implementations = mem::take(&mut draft.declaration.implementations);
        for (index, implementation) in implementations.into_iter().enumerate() {
            if let Some(intents) = draft.trait_intents.remove(&index) {
                let interface = implementation.trait_type.as_ref().expect("trait intent");
                let contract = contracts
                    .traits
                    .get_id(interface.declaration)
                    .ok_or_else(|| conflict("unresolved implementation trait"))?;
                let bindings = intents
                    .iter()
                    .map(|intent| (intent.name.as_str(), intent.id))
                    .collect::<Vec<_>>();
                draft
                    .declaration
                    .implement_trait(contract, implementation, &bindings, context)
                    .map_err(conflict)?;
                lower_override_facts(&contracts, &mut draft.declaration, contract, index, intents)?;
            } else {
                draft.declaration.implementations.push(implementation);
            }
        }
    }
    let mut catalog =
        DeclarationCatalog::declared(drafts.iter().map(|draft| &draft.declaration), context)?;
    catalog.merge(providers)?;
    // Provider constructor ownership must also be consistent for an empty batch.
    catalog.receiver_owners(None)?;
    let definitions = catalog.definitions();
    let cancel = CancellationToken::default();
    for draft in drafts.iter_mut() {
        let module = &mut draft.declaration;
        let declarations = catalog.validate_module_declarations(module)?;
        let mut targets = BTreeSet::new();
        for declaration in &declarations {
            let CallableImplementation::Native(target) = declaration.function.implementation else {
                return Err(conflict("native declaration has no direct binding target"));
            };
            targets.insert(target);
        }
        if targets != draft.bindings.keys().copied().collect() {
            return Err(conflict(
                "missing, duplicate or foreign pending native binding",
            ));
        }
        let storage = module
            .types
            .iter()
            .filter_map(|ty| {
                if let TypeDefKind::NativeStorage(layout) = ty.kind {
                    Some((ty, layout))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if storage.len() != draft.storage.len()
            || storage.iter().any(|(ty, layout)| {
                let id = definitions.lookup_root(&module.identity).and_then(|root| {
                    definitions.lookup_child(root, ty.kind.definition_kind(), &ty.name, 0)
                });
                id.and_then(|id| draft.storage.get(&id))
                    .is_none_or(|storage| storage.layout() != *layout)
            })
        {
            return Err(conflict(
                "native storage entries differ from their declared types",
            ));
        }
        // Reuse the same proof/dependency traversal used by NativeModule. Export
        // and explicit-reference owners also matter when no callable uses them.
        let trait_ids = module
            .traits
            .iter()
            .map(|contract| module.definition(DefinitionKind::Trait, &contract.name, &definitions))
            .collect::<Result<Vec<_>, _>>()
            .map_err(conflict)?;
        let closure = catalog.dependency_closure(trait_ids, &declarations, [&*module])?;
        let dependencies = &closure.catalog;
        let ids = dependencies
            .types
            .ids()
            .chain(dependencies.traits.ids())
            .chain(dependencies.declarations.ids())
            .chain(dependencies.implementations.ids())
            .chain(module.exports.values().copied())
            .chain(draft.references.iter().map(|reference| reference.id))
            .chain(draft.members.iter().map(|member| member.id));
        let mut edges = BTreeSet::new();
        for id in ids {
            let owner = definitions.resolve(id).map_err(conflict)?.module().clone();
            if owner != module.identity {
                edges.insert(owner);
            }
        }
        module.dependencies.extend(edges);
        catalog.validate_module_shape(module)?;
    }
    catalog.check_implementations(drafts.iter().map(|draft| &draft.declaration))?;
    catalog.validate_callable_contracts()?;
    catalog.validate_paths()?;
    inherit_documentation(&definitions, drafts)?;
    let modules = drafts
        .iter()
        .map(|draft| {
            DefinitionMetadata::checked(definitions.clone(), draft.declaration.clone(), &cancel)
                .map_err(conflict)
        })
        .collect::<NativeResult<Vec<_>>>()?;
    Ok(DeclarationSnapshot {
        modules: modules.into(),
        catalog,
    })
}

/// Inherit authored documentation from traits in this batch after implementation
/// expansion. External catalogs do not retain prose. Explicit implementation docs
/// take precedence, and forward references do not impose recipe ordering.
fn inherit_documentation(
    definitions: &DefinitionTable,
    drafts: &mut [DraftState],
) -> NativeResult<()> {
    let documents = drafts
        .iter()
        .flat_map(|draft| &draft.declaration.documentation)
        .map(|(id, text)| (*id, text.clone()))
        .collect::<BTreeMap<_, _>>();
    for draft in drafts {
        let module = &mut draft.declaration;
        for (index, implementation) in module.implementations.iter().enumerate() {
            let Some(interface) = &implementation.trait_type else {
                continue;
            };
            let owner = module
                .implementation_id(index, definitions)
                .map_err(conflict)?;
            for method in &implementation.methods {
                let source =
                    ModuleDecl::method_id(interface.declaration, &method.name, definitions)
                        .map_err(conflict)?;
                if let Some(text) = documents.get(&source) {
                    let target = ModuleDecl::method_id(owner, &method.name, definitions)
                        .map_err(conflict)?;
                    module
                        .documentation
                        .entry(target)
                        .or_insert_with(|| text.clone());
                }
            }
        }
    }
    Ok(())
}
