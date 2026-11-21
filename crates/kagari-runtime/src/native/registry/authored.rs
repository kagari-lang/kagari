//! Installation retains importer-scoped expectations, not providers for authored declarations.
use crate::{
    error::RuntimeError,
    module::{VerifiedProgram, admission::DefinitionAdmission},
    native::{binding::NativeResult, catalog::DeclarationCatalog, registry::NativeRegistry},
};
use kagari_bytecode::module::BytecodeModule;
use kagari_common::identity::{ModuleIdentity, table::DefinitionId};
use kagari_contract::types::{PublicItem, trait_contract, type_contract};
use std::collections::{BTreeMap, btree_map::Entry};

impl NativeRegistry {
    pub(in crate::native) fn record_authored_requirements(
        &mut self,
        importer: &ModuleIdentity,
        required: DeclarationCatalog,
    ) -> NativeResult<()> {
        match self.authored_requirements.entry(importer.clone()) {
            Entry::Occupied(mut prior) => prior.get_mut().merge(&required)?,
            Entry::Vacant(slot) => {
                slot.insert(required);
            }
        }
        Ok(())
    }

    /// All local providers are staged before this check. Only the temporary signature and
    /// metadata view sees foreign public records; native ownership and link authority do not.
    pub(in crate::native) fn validation_contracts(&self) -> NativeResult<DeclarationCatalog> {
        let mut contracts = self.catalog.clone();
        if !self.authored_requirements.is_empty() {
            let mut authored = DeclarationCatalog::in_context(self.catalog.types.context().clone());
            for required in self.authored_requirements.values() {
                authored.merge(required)?;
            }
            contracts.source_impls = authored.source_impls.clone();
            for owner in authored.authored_owners() {
                if self.catalog.contains_module(owner) {
                    return Err(RuntimeError::metadata_conflict(format!(
                        "conflicting verified/native module origin: {owner}"
                    )));
                }
            }
            for (id, ty) in authored.types.entries() {
                contracts.insert_type(id, ty.clone())?;
            }
            for (id, tr) in authored.traits.entries() {
                contracts.insert(id, tr.clone())?;
            }
        }
        Ok(contracts)
    }
}

pub(super) fn validate(
    required: &DeclarationCatalog,
    importer: &BytecodeModule,
    program: &VerifiedProgram,
    admission: &DefinitionAdmission,
) -> NativeResult<()> {
    let mut reachable = BTreeMap::new();
    let mut pending = importer.dependencies.clone();
    while let Some(slot) = pending.pop() {
        let module = &program.modules()[slot.index()];
        if reachable.insert(&module.identity, module).is_none() {
            pending.extend_from_slice(&module.dependencies);
        }
    }
    for owner in required.authored_owners() {
        if !reachable.contains_key(owner) {
            return Err(RuntimeError::module_validation(format!(
                "{} requires reachable authored owner {owner}",
                importer.identity
            )));
        }
    }
    let definitions = required.definitions();
    let source = program.definitions();
    let resolve = |id: DefinitionId| {
        let path = definitions
            .resolve(id)
            .map_err(|error| RuntimeError::module_validation(error.to_string()))?
            .to_path();
        let mismatch = || {
            RuntimeError::module_validation(format!(
                "{} requires the exact public authored contract {path:?}",
                importer.identity
            ))
        };
        let module = reachable.get(&path.module).copied().ok_or_else(mismatch)?;
        let id = source.lookup(&path).ok_or_else(mismatch)?;
        Ok::<_, RuntimeError>((module, id))
    };
    for (id, expected) in required.types.entries() {
        let (owner, id) = resolve(id)?;
        if type_contract(&owner.identity, &owner.public_items, &id, source)
            .map(|actual| admission.runtime(actual, &Default::default()))
            .transpose()?
            .as_ref()
            != Some(expected)
        {
            return Err(RuntimeError::module_validation(format!(
                "{} requires the exact public authored type {}::{}",
                importer.identity, owner.identity, expected.name
            )));
        }
    }
    for (id, expected) in required.source_impls.entries() {
        let (owner, id) = resolve(id)?;
        let actual = owner.public_items.iter().find_map(|item| match item {
            PublicItem::InterfaceTable(table) if table.declaration == id => Some(table.as_ref()),
            _ => None,
        });
        if actual
            .map(|table| admission.runtime(table, &Default::default()))
            .transpose()?
            .as_ref()
            != Some(expected)
        {
            return Err(RuntimeError::module_validation(format!(
                "{} requires the exact authored implementation {} in {}",
                importer.identity, expected.name, owner.identity
            )));
        }
    }
    for (id, expected) in required.traits.entries() {
        let (owner, id) = resolve(id)?;
        if trait_contract(&owner.identity, &owner.public_items, &[], &id, source)
            .map(|actual| admission.runtime(actual, &Default::default()))
            .transpose()?
            .as_ref()
            != Some(expected)
        {
            return Err(RuntimeError::module_validation(format!(
                "{} requires the exact public authored trait {}::{}",
                importer.identity, owner.identity, expected.name
            )));
        }
    }
    Ok(())
}
