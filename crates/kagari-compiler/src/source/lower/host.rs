use crate::source::lower::MirLoweringError;
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::{DefinitionId, DefinitionTable},
};
use kagari_contract::host as module_host;
use kagari_hir::AnalyzedModule;
use kagari_mir::{
    function::MirFunction,
    instruction::{CallTarget, Instruction},
};
use kagari_types::declaration::TypeDeclaration;
use kagari_types::host_interface::{
    path::HostPathSegmentDeclaration, type_declaration::HostTypeDeclaration,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    iter,
};

pub(super) struct Dependencies {
    pub legacy: Vec<HostTypeDeclaration>,
    pub shared: Vec<TypeDeclaration>,
}

pub(super) fn collect(
    module: &AnalyzedModule,
    roots: BTreeSet<DefinitionId>,
    functions: &[MirFunction],
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<Dependencies, MirLoweringError> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    for instruction in functions
        .iter()
        .flat_map(|f| &f.blocks)
        .flat_map(|b| &b.instructions)
    {
        cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
        if let Some(declaration) = instruction
            .path_reference()
            .and_then(|path| path.declaration.as_ref())
        {
            pending.push(declaration.root);
            for segment in &declaration.segments {
                match segment {
                    HostPathSegmentDeclaration::Field(field) => {
                        let owner = definitions
                            .parent(*field)
                            .ok()
                            .flatten()
                            .ok_or(MirLoweringError::MissingBinding("host field owner"))?;
                        pending.push(owner);
                    }
                    HostPathSegmentDeclaration::Index(index) => {
                        for ty in [&index.collection, &index.index, &index.result] {
                            pending.extend(ty.nominal_references().into_iter().cloned());
                        }
                    }
                    HostPathSegmentDeclaration::Virtual(virtual_step) => {
                        pending.extend(
                            virtual_step
                                .result
                                .nominal_references()
                                .into_iter()
                                .cloned(),
                        );
                    }
                }
            }
        }
        if let Instruction::Call {
            callee: CallTarget::Native(import),
            ..
        } = instruction
        {
            pending.extend(
                module_host::type_references(
                    import
                        .signature
                        .params
                        .iter()
                        .chain(iter::once(&import.signature.result)),
                    cancel,
                )
                .map_err(|_| MirLoweringError::Cancelled)?,
            );
        }
    }
    let mut declarations = BTreeMap::new();
    let mut shared = BTreeMap::new();
    while let Some(id) = pending.pop() {
        cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
        if declarations.contains_key(&id) || shared.contains_key(&id) {
            continue;
        }
        if let Some(definition) = module.declarations.external_type(id) {
            // Common schemas remain the semantic owner, including recursive members.
            // Only external identities enter this closure, not ordinary DTO nominals.
            for field in &definition.fields {
                pending.extend(
                    module_host::type_references([&field.ty], cancel)
                        .map_err(|_| MirLoweringError::Cancelled)?,
                );
            }
            shared.insert(
                id,
                TypeDeclaration {
                    declaration: id,
                    definition: definition.clone(),
                },
            );
            continue;
        }
        let hosts = &module.names.hosts;
        let ty = hosts
            .nominal_type(&id)
            .and_then(|id| hosts.type_declaration(id))
            .ok_or(MirLoweringError::MissingBinding(
                "nominal host type declaration",
            ))?;
        for member in ty.value_types() {
            pending.extend(member.nominal_references().into_iter().cloned());
        }
        declarations.insert(id, ty.clone());
    }
    // IDs deduplicate the closure, but allocation history must not order emitted contracts.
    let mut declarations: Vec<_> = declarations.into_values().collect();
    declarations.sort_by(|left, right| {
        definitions
            .compare(left.id, right.id)
            .expect("checked Host type identities")
    });
    cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
    let mut shared: Vec<_> = shared.into_values().collect();
    shared.sort_by(|left, right| {
        definitions
            .compare(left.declaration, right.declaration)
            .expect("checked external type identities")
    });
    Ok(Dependencies {
        legacy: declarations,
        shared,
    })
}
