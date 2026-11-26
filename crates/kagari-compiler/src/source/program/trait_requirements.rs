//! Complete dependencies and portable trait requirements before candidate verification.

use super::{InstallationTraitRequirements, SourceProgramError};
use crate::source::lower::MirLoweringError;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        mapping::{DefinitionMappingError, check_cancel},
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::types::{PublicItem, TraitContract};
use kagari_mir::function::MirModule;
use kagari_types::{callable::CallableImplementation, declaration::TraitDef, language::Protocol};
use std::collections::{BTreeSet, HashMap, HashSet};

struct TraitOwner<'a> {
    abi: &'a TraitDef,
    public: bool,
}

pub(super) fn attach(
    modules: &mut [MirModule],
    installation: &InstallationTraitRequirements,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), SourceProgramError> {
    let error = |module: &ModuleIdentity, error: MirLoweringError| SourceProgramError::Lowering {
        module: Box::new(module.clone()),
        error,
    };
    let mut owners = HashMap::new();
    let source_owners: HashSet<_> = modules.iter().map(|module| &module.identity).collect();
    for module in modules.iter() {
        let root = definitions
            .lookup_root(&module.identity)
            .expect("checked module");
        for item in &module.abi.public_items {
            check_cancel(cancel).map_err(|e| error(&module.identity, e.into()))?;
            if let PublicItem::Trait(abi) = item {
                let id = definitions
                    .lookup_child(root, DefinitionKind::Trait, &abi.name, 0)
                    .expect("checked public trait");
                owners.insert(id, TraitOwner { abi, public: true });
            }
        }
        for contract in &module.abi.trait_contracts {
            owners.insert(
                contract.declaration,
                TraitOwner {
                    abi: &contract.abi,
                    public: false,
                },
            );
        }
    }
    for importer in installation.by_importer.keys() {
        if !source_owners.contains(importer) {
            return Err(error(
                importer,
                MirLoweringError::MissingBinding("installation trait importer"),
            ));
        }
    }
    let mut products = Vec::with_capacity(modules.len());
    for module in modules.iter() {
        products.push(
            collect(
                module,
                installation,
                &owners,
                &source_owners,
                definitions,
                cancel,
            )
            .map_err(|e| error(&module.identity, e))?,
        );
    }
    for (module, (references, dependencies)) in modules.iter_mut().zip(products) {
        module.abi.trait_references = references;
        module.dependencies = dependencies;
    }
    Ok(())
}

fn collect(
    module: &MirModule,
    installation: &InstallationTraitRequirements,
    owners: &HashMap<DefinitionId, TraitOwner<'_>>,
    source_owners: &HashSet<&ModuleIdentity>,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(Vec<TraitContract>, Vec<ModuleIdentity>), MirLoweringError> {
    let mut required = HashSet::new();
    let mut dependencies: BTreeSet<_> = module.dependencies.iter().cloned().collect();
    let mut retain_owner = |owner: &ModuleIdentity| {
        if owner != &module.identity && source_owners.contains(owner) {
            dependencies.insert(owner.clone());
        }
    };
    // Scan the actual products before attaching requirements. This includes every
    // ABI bound and Native callable operation, plus uses from instantiated MIR.
    // Generic arguments and witnesses can also introduce module dependencies.
    module.visit_definition_ids(
        &mut |id| {
            let owner = definitions.resolve(*id)?.module();
            if owner != &module.identity {
                retain_owner(owner);
                if let Some(id) = enclosing_trait(*id, definitions)? {
                    required.insert(id);
                }
            }
            Ok(())
        },
        cancel,
    )?;
    let mut references = Vec::new();
    for id in required {
        check_cancel(cancel)?;
        // Protocols and Native adapters have separate executable contracts. The
        // portable source-trait expectation cannot authorize their dispatch.
        if Protocol::from_definition(&id, definitions).is_some() {
            continue;
        }
        let abi = owners
            .get(&id)
            .ok_or(DefinitionMappingError::InvalidContract)?
            .abi;
        if abi.conversion_adapter.is_none()
            && abi.methods.iter().all(|method| {
                method.host_policy.is_none()
                    && !matches!(method.implementation, CallableImplementation::Native(_))
            })
        {
            references.push(TraitContract {
                declaration: id,
                abi: abi.clone(),
            });
        }
    }
    if let Some(paths) = installation.by_importer.get(&module.identity) {
        let inferred: HashSet<_> = references.iter().map(|r| r.declaration).collect();
        for path in paths {
            check_cancel(cancel)?;
            // Lexical access can justify an inferred private reference, not a public pin.
            let (id, owner) = definitions
                .lookup(path)
                .and_then(|id| Some((id, owners.get(&id)?)))
                .filter(|(_, owner)| owner.public && path.module != module.identity)
                .ok_or_else(|| MirLoweringError::MissingHostDeclaration(path.clone()))?;
            // Mandatory identities cannot be silently filtered out. The portable
            // verifier rejects an ineligible expectation, just as on decode.
            if !inferred.contains(&id) {
                references.push(TraitContract {
                    declaration: id,
                    abi: owner.abi.clone(),
                });
            }
        }
    }
    // These records can mention additional owners, but those mentions aren't
    // independent uses by the importer. Complete only the dependency edges.
    for reference in &references {
        reference.visit_definition_ids(
            &mut |id| {
                retain_owner(definitions.resolve(*id)?.module());
                Ok(())
            },
            cancel,
        )?;
    }
    references.sort_unstable_by(|a, b| {
        definitions
            .compare(a.declaration, b.declaration)
            .expect("checked trait identities")
    });
    Ok((references, dependencies.into_iter().collect()))
}

/// A use may name just a method, an associated member, or a binder below one.
fn enclosing_trait(
    mut id: DefinitionId,
    definitions: &DefinitionTable,
) -> Result<Option<DefinitionId>, DefinitionMappingError> {
    loop {
        if definitions
            .resolve(id)?
            .last()
            .is_some_and(|s| s.kind == DefinitionKind::Trait)
        {
            return Ok(Some(id));
        }
        let Some(parent) = definitions.parent(id)? else {
            return Ok(None);
        };
        id = parent;
    }
}
