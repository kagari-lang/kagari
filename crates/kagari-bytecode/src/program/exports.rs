//! Verify and bind public declarations to exact executable slots once per Program.

use crate::{
    program::{BytecodeProgram, ModuleRef},
    verifier::BytecodeVerificationError,
};
use kagari_common::cancellation::CancellationToken;
use kagari_common::identity::{
    scoped::DefinitionReferences,
    table::{DefinitionId, DefinitionTable},
};
use kagari_contract::{
    exports::{
        entry_function_matches, public_function_matches, public_generic_function_matches,
        type_validation::{TypeExportModule, TypeExportValidator},
    },
    ids::FunctionRef,
    types::proofs::ProofCatalog,
};
use std::collections::{HashMap, HashSet};

/// A concrete callable slot, meaningful only in its supplying verified Program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportTarget {
    pub module: ModuleRef,
    pub function: FunctionRef,
}

pub(crate) fn verify(
    program: &BytecodeProgram,
    definitions: &DefinitionTable,
    catalog: &ProofCatalog<'_>,
    cancel: &CancellationToken,
) -> Result<HashMap<DefinitionId, ExportTarget>, BytecodeVerificationError> {
    let invalid = || {
        if cancel.check().is_err() {
            BytecodeVerificationError::Cancelled
        } else {
            BytecodeVerificationError::InvalidProgramExports
        }
    };
    let required = program
        .exports
        .namespaces
        .values()
        .flat_map(|namespace| namespace.functions.values().copied())
        .chain(program.exports.entry)
        .collect::<HashSet<_>>();
    program
        .exports
        .visit_definition_ids(
            &mut |id| {
                definitions.resolve(*id)?;
                Ok(())
            },
            cancel,
        )
        .map_err(|_| invalid())?;
    let mut bindings = HashMap::new();
    let mut generic_declarations = HashSet::new();
    for (index, module) in program.modules.iter().enumerate() {
        for record in &module.named_declarations {
            cancel
                .check()
                .map_err(|_| BytecodeVerificationError::Cancelled)?;
            if public_generic_function_matches(
                record,
                &module.identity,
                &module.public_items,
                definitions,
            ) {
                generic_declarations.insert(record.declaration);
            }
        }
        for function in &module.functions {
            cancel
                .check()
                .map_err(|_| BytecodeVerificationError::Cancelled)?;
            let Some(identity) = &function.identity else {
                continue;
            };
            if !required.contains(&identity.declaration)
                || !identity.arguments.is_empty()
                || function.metadata.semantic.generic.is_some()
            {
                continue;
            }
            if definitions
                .resolve(identity.declaration)
                .map_err(|_| invalid())?
                .module()
                != &module.identity
                || bindings
                    .insert(
                        identity.declaration,
                        ExportTarget {
                            module: ModuleRef::new(index),
                            function: function.id,
                        },
                    )
                    .is_some()
            {
                return Err(invalid());
            }
        }
    }
    let types = TypeExportValidator::new(
        program.modules.iter().map(|module| TypeExportModule {
            identity: &module.identity,
            public_items: &module.public_items,
            private_types: &module.private_types,
            structures: &module.structures,
        }),
        catalog,
        cancel,
    )
    .map_err(|_| invalid())?;
    program
        .exports
        .validate(
            &program.modules[program.root.index()].identity,
            definitions,
            cancel,
            |id, entry| {
                if !entry && generic_declarations.contains(&id) {
                    return true;
                }
                let Some(target) = bindings.get(&id) else {
                    return false;
                };
                let owner = &program.modules[target.module.index()];
                let function = &owner.functions[target.function.index()];
                if entry {
                    function.parameter_count == 0
                        && entry_function_matches(
                            id,
                            &owner.public_items,
                            &owner.native_declarations,
                            &function.metadata.semantic,
                            definitions,
                        )
                } else {
                    public_function_matches(
                        id,
                        &owner.identity,
                        &owner.public_items,
                        &function.metadata.semantic,
                        definitions,
                    )
                }
            },
            |export| {
                #[cfg(test)]
                super::type_exports::before_type_validation(cancel);
                types.validate(export).is_ok()
            },
        )
        .map_err(|_| invalid())?;
    Ok(bindings)
}
