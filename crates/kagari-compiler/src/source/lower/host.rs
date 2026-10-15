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
use kagari_types::declaration::{TypeDeclaration, path::HostPathSegmentDeclaration};
use std::{
    collections::{BTreeMap, BTreeSet},
    iter,
};

pub(super) fn collect(
    module: &AnalyzedModule,
    modules: &[AnalyzedModule],
    roots: BTreeSet<DefinitionId>,
    functions: &[MirFunction],
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<Vec<TypeDeclaration>, MirLoweringError> {
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
                            pending.extend(
                                module_host::type_references([ty], cancel)
                                    .map_err(|_| MirLoweringError::Cancelled)?,
                            );
                        }
                    }
                    HostPathSegmentDeclaration::Virtual(virtual_step) => {
                        pending.extend(
                            module_host::type_references([&virtual_step.result], cancel)
                                .map_err(|_| MirLoweringError::Cancelled)?,
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
    let mut shared = BTreeMap::new();
    while let Some(id) = pending.pop() {
        cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
        if shared.contains_key(&id) {
            continue;
        }
        let owner = definitions
            .resolve(id)
            .map_err(|error| MirLoweringError::Identity(error.into()))?
            .module();
        let definition = module.declarations.external_type(id).or_else(|| {
            // Concrete generic bodies may mention a host type absent from this
            // module's source imports. Its checked defining module owns the schema.
            modules
                .iter()
                .find(|provider| provider.lowered.source.module_identity() == owner)
                .and_then(|provider| provider.declarations.external_type(id))
        });
        if let Some(definition) = definition {
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
        return Err(definitions
            .resolve(id)
            .map(|view| MirLoweringError::MissingHostDeclaration(view.to_path()))
            .unwrap_or_else(|error| MirLoweringError::Identity(error.into())));
    }
    // IDs deduplicate the closure, but allocation history must not order emitted contracts.
    cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
    let mut shared: Vec<_> = shared.into_values().collect();
    shared.sort_by(|left, right| {
        definitions
            .compare(left.declaration, right.declaration)
            .expect("checked external type identities")
    });
    Ok(shared)
}
