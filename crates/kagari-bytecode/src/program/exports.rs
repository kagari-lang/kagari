//! Verify and bind public declarations to exact executable slots once per Program.

use crate::{
    program::{BytecodeProgram, ModuleRef},
    verifier::BytecodeVerificationError,
};
use kagari_common::identity::{
    scoped::DefinitionReferences,
    table::{DefinitionId, DefinitionTable},
};
use kagari_contract::{
    exports::{entry_function_matches, public_function_matches},
    ids::FunctionRef,
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
) -> Result<HashMap<DefinitionId, ExportTarget>, BytecodeVerificationError> {
    let invalid = || BytecodeVerificationError::InvalidProgramExports;
    let cancel = Default::default();
    let mut required = HashSet::new();
    program
        .exports
        .visit_definition_ids(
            &mut |id| {
                definitions.resolve(*id)?;
                required.insert(*id);
                Ok(())
            },
            &cancel,
        )
        .map_err(|_| invalid())?;
    let mut bindings = HashMap::new();
    for (index, module) in program.modules.iter().enumerate() {
        for function in &module.functions {
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
    program
        .exports
        .validate(
            &program.modules[program.root.index()].identity,
            definitions,
            &cancel,
            |id, entry| {
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
        )
        .map_err(|_| invalid())?;
    Ok(bindings)
}
