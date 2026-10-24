//! Directional preservation of authored contracts, independent of executable slots.

use crate::reload::{ReloadValidationError, identity_error};
use kagari_bytecode::module::BytecodeModule;
use kagari_common::identity::table::{DefinitionId, DefinitionTable};
use kagari_contract::types::{
    InterfaceTable, PublicItem,
    named::{
        ImplementationContract, NamedContract, NamedDeclaration,
        correspondence::DeclarationCorrespondence,
    },
};
use kagari_types::{
    declaration::{FnDecl, TypeDeclaration},
    ty::Ty,
    visibility::Visibility,
};
use std::collections::{HashMap, HashSet};

pub(super) fn validate(
    previous: &[BytecodeModule],
    candidate: &[BytecodeModule],
    previous_definitions: &DefinitionTable,
    candidate_definitions: &DefinitionTable,
) -> Result<(), ReloadValidationError> {
    let mut correspondence = DeclarationCorrespondence::new().map_err(identity_error)?;
    let cancel = Default::default();
    let old_types = correspondence
        .import(
            &nominal_declarations(previous, previous_definitions)?,
            inventory(previous),
            previous_definitions,
            &cancel,
        )
        .map_err(identity_error)?;
    let new_types = correspondence
        .import(
            &nominal_declarations(candidate, candidate_definitions)?,
            inventory(candidate),
            candidate_definitions,
            &cancel,
        )
        .map_err(identity_error)?;
    let old_types = old_types
        .records()
        .iter()
        .map(|record| record.declaration)
        .collect::<HashSet<_>>();
    let new_types = new_types
        .records()
        .iter()
        .map(|record| record.declaration)
        .collect::<HashSet<_>>();
    let old = correspondence
        .import(
            &previous.to_vec(),
            inventory(previous),
            previous_definitions,
            &cancel,
        )
        .map_err(identity_error)?;
    let new = correspondence
        .import(
            &candidate.to_vec(),
            inventory(candidate),
            candidate_definitions,
            &cancel,
        )
        .map_err(identity_error)?;
    let definitions = new.definitions();

    let old_modules = old
        .records()
        .iter()
        .map(|module| (&module.identity, module))
        .collect::<HashMap<_, _>>();
    let new_modules = new
        .records()
        .iter()
        .map(|module| (&module.identity, module))
        .collect::<HashMap<_, _>>();
    let old_impls = old
        .records()
        .iter()
        .flat_map(|module| &module.named_declarations)
        .filter_map(|record| {
            matches!(record.contract, NamedContract::Implementation(_))
                .then_some(record.declaration)
        })
        .collect::<HashSet<_>>();
    for module in new.records() {
        for record in &module.named_declarations {
            if let NamedContract::Implementation(header) = &record.contract
                && header.trait_type.is_some()
                && !old_impls.contains(&record.declaration)
            {
                let nominal = match &header.receiver {
                    Ty::Struct(nominal) | Ty::Enum(nominal) | Ty::NativeObject(nominal) => {
                        Some(nominal.declaration)
                    }
                    Ty::Host(id) => Some(*id),
                    _ => None,
                };
                if nominal.is_none_or(|id| old_types.contains(&id) || !new_types.contains(&id)) {
                    return Err(changed(record.declaration, definitions));
                }
            }
        }
    }
    let same_module_indices = old.records().len() <= new.records().len()
        && old
            .records()
            .iter()
            .zip(new.records())
            .all(|(left, right)| left.identity == right.identity);
    for (identity, previous) in old_modules {
        let candidate = new_modules.get(identity).ok_or_else(invalid)?;
        preserve_ambiguous(previous, candidate, definitions, same_module_indices)?;
        preserve(
            &previous.named_declarations,
            &candidate.named_declarations,
            |record| record.declaration,
            definitions,
        )?;
        preserve(
            &previous.private_types,
            &candidate.private_types,
            |record| record.declaration,
            definitions,
        )?;
        preserve(
            &previous.trait_contracts,
            &candidate.trait_contracts,
            |record| record.declaration,
            definitions,
        )?;
        preserve(
            &previous.native_declarations,
            &candidate.native_declarations,
            |record| record.declaration,
            definitions,
        )?;
        for slot in &previous.module_slots {
            let candidate = candidate
                .module_slots
                .iter()
                .find(|other| other.contract.declaration == slot.contract.declaration);
            if candidate.is_none_or(|other| other.contract != slot.contract) {
                return Err(changed(slot.contract.declaration, definitions));
            }
        }
        for item in &previous.public_items {
            match item {
                PublicItem::InterfaceTable(table) => {
                    let candidate = candidate
                        .public_items
                        .iter()
                        .find_map(|item| match item {
                            PublicItem::InterfaceTable(other)
                                if other.declaration == table.declaration =>
                            {
                                Some(other.as_ref())
                            }
                            _ => None,
                        })
                        .ok_or_else(invalid)?;
                    if !interface_preserved(table, candidate) {
                        return Err(invalid());
                    }
                }
                PublicItem::InherentTable(table) => {
                    // Equivalent inherent blocks form one authored owner. Members
                    // may be split or merged without changing their identities.
                    let methods = candidate
                        .public_items
                        .iter()
                        .filter_map(|item| match item {
                            PublicItem::InherentTable(other)
                                if other.declaration == table.declaration =>
                            {
                                Some(&other.methods)
                            }
                            _ => None,
                        })
                        .flatten()
                        .collect::<Vec<_>>();
                    if table
                        .methods
                        .iter()
                        .any(|method| !methods.contains(&method))
                    {
                        return Err(invalid());
                    }
                }
                _ if !candidate.public_items.contains(item) => return Err(invalid()),
                _ => {}
            }
        }
        if previous
            .paths
            .iter()
            .any(|path| !candidate.paths.contains(path))
        {
            return Err(ReloadValidationError::PathFingerprintMismatch);
        }
    }
    Ok(())
}

// Public TypeDef identities are implicit names, so an unused nongeneric type may
// have no explicit reference in the module. Import its verified ID as well rather
// than trying to reconstruct it from an incomplete comparison scope.
fn nominal_declarations(
    modules: &[BytecodeModule],
    definitions: &DefinitionTable,
) -> Result<Vec<TypeDeclaration>, ReloadValidationError> {
    let mut declarations = Vec::new();
    for module in modules {
        declarations.extend(module.private_types.iter().cloned());
        for item in &module.public_items {
            if let PublicItem::Type(ty) = item {
                let root = definitions
                    .lookup_root(&module.identity)
                    .ok_or_else(invalid)?;
                declarations.push(TypeDeclaration {
                    declaration: definitions
                        .lookup_child(root, ty.kind.definition_kind(), &ty.name, 0)
                        .ok_or_else(invalid)?,
                    definition: ty.clone(),
                });
            }
        }
    }
    Ok(declarations)
}

pub(super) fn inventory<'a>(
    modules: impl IntoIterator<Item = &'a BytecodeModule>,
) -> impl Iterator<Item = NamedDeclaration> {
    modules.into_iter().flat_map(|module| {
        module
            .named_declarations
            .iter()
            .cloned()
            .chain(module.public_items.iter().filter_map(|item| {
                let PublicItem::InterfaceTable(table) = item else {
                    return None;
                };
                table.bridge?;
                Some(NamedDeclaration {
                    declaration: table.declaration,
                    visibility: Visibility::Private,
                    ambiguous_local_body: None,
                    contract: NamedContract::Implementation(ImplementationContract {
                        bridge: table.bridge,
                        generic_params: table.generic_params.clone(),
                        bounds: table.bounds.clone(),
                        receiver: table.for_type.clone(),
                        trait_type: Some(table.trait_type.clone()),
                    }),
                })
            }))
    })
}

fn preserve_ambiguous(
    previous: &BytecodeModule,
    candidate: &BytecodeModule,
    definitions: &DefinitionTable,
    same_module_indices: bool,
) -> Result<(), ReloadValidationError> {
    let mut owners = HashSet::new();
    for record in &previous.named_declarations {
        if record.ambiguous_local_body.is_some() {
            owners.insert(
                definitions
                    .parent(record.declaration)
                    .map_err(|error| identity_error(error.into()))?
                    .ok_or_else(invalid)?,
            );
        }
    }
    for owner in owners {
        // Exact indexed evidence is deliberately conservative: if unrelated edits
        // renumber an operand table, unchanged text alone cannot establish which
        // same-name local an old operand denotes. Do not guess correspondence.
        if !same_module_indices
            || previous.constants != candidate.constants
            || previous.function_table != candidate.function_table
            || previous.native_imports != candidate.native_imports
            || previous.structures != candidate.structures
            || previous.enumerations != candidate.enumerations
            || previous.interface_tables != candidate.interface_tables
            || previous.module_slots != candidate.module_slots
            || previous.paths != candidate.paths
        {
            return Err(changed(owner, definitions));
        }
        let initializer = previous
            .module_slots
            .iter()
            .find(|slot| slot.contract.declaration == owner)
            .map(|slot| slot.initializer);
        for body in &previous.functions {
            let Some(identity) = &body.identity else {
                continue;
            };
            if definitions.contains(owner, identity.declaration) != Ok(true)
                && initializer != Some(body.id)
            {
                continue;
            }
            let Some(current) = candidate
                .functions
                .iter()
                .find(|body| body.identity.as_ref() == Some(identity))
            else {
                return Err(changed(owner, definitions));
            };
            if body.instructions != current.instructions
                || body.metadata.semantic != current.metadata.semantic
                || body.parameter_count != current.parameter_count
                || body.slot_count != current.slot_count
                || body.metadata.params != current.metadata.params
                || body.metadata.slots != current.metadata.slots
                || body.metadata.return_type != current.metadata.return_type
                || body.metadata.parameter_slots != current.metadata.parameter_slots
            {
                return Err(changed(owner, definitions));
            }
        }
    }
    Ok(())
}

fn interface_preserved(previous: &InterfaceTable, candidate: &InterfaceTable) -> bool {
    previous.associated_type_families == candidate.associated_type_families
        && previous.associated_consts == candidate.associated_consts
        && previous.bridge == candidate.bridge
        && previous.generic_params == candidate.generic_params
        && previous.bounds == candidate.bounds
        && previous.trait_type == candidate.trait_type
        && previous.for_type == candidate.for_type
        && same_methods(&previous.methods, &candidate.methods)
}

fn same_methods(previous: &[FnDecl], candidate: &[FnDecl]) -> bool {
    previous.len() == candidate.len() && previous.iter().all(|method| candidate.contains(method))
}

fn preserve<T: PartialEq>(
    previous: &[T],
    candidate: &[T],
    declaration: impl Fn(&T) -> DefinitionId,
    definitions: &DefinitionTable,
) -> Result<(), ReloadValidationError> {
    let mut current = HashMap::new();
    for record in candidate {
        let id = declaration(record);
        if let Some(other) = current.insert(id, record)
            && other != record
        {
            return Err(changed(id, definitions));
        }
    }
    for record in previous {
        let id = declaration(record);
        if current.get(&id).copied() != Some(record) {
            return Err(changed(id, definitions));
        }
    }
    Ok(())
}

fn changed(id: DefinitionId, definitions: &DefinitionTable) -> ReloadValidationError {
    let declaration = definitions
        .resolve(id)
        .map(|view| {
            let path = view.to_path();
            format!(
                "{}::{}",
                path.module,
                path.path
                    .iter()
                    .map(|part| part.name.as_str())
                    .collect::<Vec<_>>()
                    .join("::")
            )
        })
        .unwrap_or_else(|_| "invalid declaration identity".into());
    ReloadValidationError::DeclarationContractMismatch { declaration }
}

fn invalid() -> ReloadValidationError {
    ReloadValidationError::PublicAbiFingerprintMismatch
}
