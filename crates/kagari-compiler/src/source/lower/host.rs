use crate::source::lower::{MirLoweringError, ModuleLoweringInputs};
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::{DefinitionId, DefinitionTable, module::ModuleHandle},
};
use kagari_contract::host as module_host;
use kagari_hir::compile_input::program::CheckedModule;
use kagari_mir::function::MirModule;
use kagari_types::declaration::{
    TypeDeclaration,
    path::{HostPathDeclaration, HostPathSegmentDeclaration},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    iter,
};

pub(super) fn collect(
    inputs: &ModuleLoweringInputs<'_>,
    roots: &BTreeSet<DefinitionId>,
    mir: &MirModule,
    cancel: &CancellationToken,
) -> Result<Vec<TypeDeclaration>, MirLoweringError> {
    let definitions = inputs.context.snapshot();
    let mut pending: Vec<_> = roots.iter().copied().collect();
    pending.extend(
        module_host::references(
            &mir.abi.public_items,
            &mir.structures,
            &mir.enumerations,
            cancel,
        )
        .map_err(|_| MirLoweringError::Cancelled)?,
    );
    let signatures = mir.abi.native_declarations.iter().flat_map(|decl| {
        decl.function
            .params
            .iter()
            .map(|param| &param.ty)
            .chain(iter::once(&decl.function.return_type))
    });

    let slots = mir.functions.iter().flat_map(|function| {
        function
            .semantic
            .params
            .values()
            .chain(function.semantic.result.iter())
            .chain(function.semantic.locals.values())
            .chain(function.semantic.registers.values())
    });
    pending.extend(
        module_host::type_references(signatures.chain(slots), cancel)
            .map_err(|_| MirLoweringError::Cancelled)?,
    );

    // An operation's intermediate Host types need not occur in any call signature.
    let mut demanded = BTreeSet::new();
    for import in mir.native_applications() {
        cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
        demanded.insert(import.instance.declaration);
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
    let imported = demanded
        .iter()
        .filter_map(|id| inputs.native_declarations.get(id));
    for declaration in mir.abi.native_declarations.iter().chain(imported) {
        if let Some(operation) = &declaration.path_operation {
            path_roots(&operation.path, &mut pending, &definitions, cancel)?;
        }
    }
    let instructions = mir
        .functions
        .iter()
        .flat_map(|f| &f.blocks)
        .flat_map(|b| &b.instructions);

    for instruction in instructions {
        cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
        if let Some(path) = instruction
            .path_reference()
            .and_then(|path| path.declaration.as_ref())
        {
            path_roots(path, &mut pending, &definitions, cancel)?;
        }
    }
    schema_closure(&inputs.modules_by_handle, pending, &definitions, cancel)
}

fn path_roots(
    declaration: &HostPathDeclaration,
    pending: &mut Vec<DefinitionId>,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), MirLoweringError> {
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
                pending.extend(
                    module_host::type_references(
                        [&index.collection, &index.index, &index.result],
                        cancel,
                    )
                    .map_err(|_| MirLoweringError::Cancelled)?,
                );
            }
            HostPathSegmentDeclaration::Virtual(virtual_step) => {
                pending.extend(
                    module_host::type_references([&virtual_step.result], cancel)
                        .map_err(|_| MirLoweringError::Cancelled)?,
                );
            }
        }
    }
    Ok(())
}

pub(super) fn schema_closure(
    modules: &HashMap<ModuleHandle, &CheckedModule<'_>>,
    mut pending: Vec<DefinitionId>,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<Vec<TypeDeclaration>, MirLoweringError> {
    let mut shared = BTreeMap::new();
    while let Some(id) = pending.pop() {
        cancel.check().map_err(|_| MirLoweringError::Cancelled)?;
        if shared.contains_key(&id) {
            continue;
        }
        let owner = definitions
            .module_of(id)
            .map_err(|error| MirLoweringError::Identity(error.into()))?;
        let schema = modules.get(&owner).and_then(|module| {
            module
                .registered_host_schemas()
                .iter()
                .find(|ty| ty.declaration == id)
        });
        if let Some(schema) = schema {
            // Common schemas remain the semantic owner, including recursive members.
            // Only external identities enter this closure, not ordinary DTO nominals.
            for field in &schema.definition.fields {
                pending.extend(
                    module_host::type_references([&field.ty], cancel)
                        .map_err(|_| MirLoweringError::Cancelled)?,
                );
            }
            shared.insert(id, schema.clone());
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
