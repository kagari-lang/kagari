//! Verify explicit imported-local placement against authored callable contracts.

use crate::{
    decode_limits::{MAX_FUNCTIONS, MAX_TABLE_RECORDS},
    slots::FunctionTypeFacts,
    types::{
        ConcreteFunctionIdentity,
        named::{
            FunctionOrigin, NamedContract, NamedDeclaration,
            verify::{SignatureScope, signature_matches_with_lookup},
        },
        proofs::ProofCatalog,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::ty::substitution::TypeTransformError;
use std::collections::{HashMap, HashSet};

/// Verify every placed application against its reachable authored local contract.
/// `lookup` must search only the receiving module's checked dependency closure.
/// Origin metadata is not a capture/body proof and never marks an actual closure
/// as a named function merely because it has zero captures.
pub fn validate<'a, S: FunctionTypeFacts + 'a>(
    catalog: &ProofCatalog<'_>,
    origins: &[FunctionOrigin],
    module: &ModuleIdentity,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
    functions: impl IntoIterator<Item = (&'a ConcreteFunctionIdentity, &'a S)>,
    lookup: impl Fn(DefinitionId) -> Option<&'a NamedDeclaration>,
) -> Result<(), TypeTransformError> {
    let invalid = TypeTransformError::InvalidContract;
    if origins.len() > MAX_FUNCTIONS {
        return Err(invalid);
    }
    let mut bodies = HashMap::<_, Vec<_>>::new();
    let mut remaining = MAX_TABLE_RECORDS;
    for (identity, semantic) in functions {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        remaining = remaining.checked_sub(1).ok_or(invalid)?;
        if !signature_matches_with_lookup(
            SignatureScope::Linked(catalog),
            identity,
            semantic,
            definitions,
            cancel,
            &lookup,
        ) {
            return Err(invalid);
        }
        bodies
            .entry(identity.declaration)
            .or_default()
            .push((identity, semantic));
    }
    let mut seen = HashSet::new();
    for origin in origins {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        remaining = remaining.checked_sub(1).ok_or(invalid)?;
        let placement = definitions.resolve(origin.placement).map_err(|_| invalid)?;
        let authored = definitions.resolve(origin.authored).map_err(|_| invalid)?;
        let parent = definitions
            .parent(origin.authored)
            .map_err(|_| invalid)?
            .ok_or(invalid)?;
        if placement.module() != module
            || authored.module() == module
            || !seen.insert(origin.placement)
            || !placement
                .last()
                .is_some_and(|part| part.kind == DefinitionKind::Function)
            || !authored
                .last()
                .is_some_and(|part| part.kind == DefinitionKind::Function)
            || !definitions
                .resolve(parent)
                .map_err(|_| invalid)?
                .last()
                .is_some_and(|part| {
                    matches!(
                        part.kind,
                        DefinitionKind::Function
                            | DefinitionKind::Method
                            | DefinitionKind::Static
                            | DefinitionKind::Const
                    )
                })
        {
            return Err(invalid);
        }
        let declaration = lookup(origin.authored).ok_or(invalid)?;
        if !matches!(declaration.contract, NamedContract::Function(_)) {
            return Err(invalid);
        }
        for (identity, semantic) in bodies.get(&origin.placement).ok_or(invalid)? {
            remaining = remaining.checked_sub(1).ok_or(invalid)?;
            let identity = ConcreteFunctionIdentity {
                declaration: origin.authored,
                arguments: identity.arguments.clone(),
            };
            if !signature_matches_with_lookup(
                SignatureScope::Linked(catalog),
                &identity,
                *semantic,
                definitions,
                cancel,
                &lookup,
            ) {
                return Err(invalid);
            }
        }
    }
    Ok(())
}
