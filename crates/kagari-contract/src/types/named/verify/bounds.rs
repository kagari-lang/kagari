//! Executable witness requirements may consume only instantiated authored authority.

use crate::{
    decode_limits::MAX_NESTED_RECORDS,
    types::named::{NamedContract, NamedDeclaration},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::ty::{
    GenericBound,
    substitution::{TypeSubstitution, TypeTransformError},
};

pub(super) fn requirements_match<'a>(
    declaration: &'a NamedDeclaration,
    executable: &[GenericBound],
    substitution: &TypeSubstitution<'_>,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
    lookup: &impl Fn(DefinitionId) -> Option<&'a NamedDeclaration>,
) -> Result<bool, TypeTransformError> {
    cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
    // Dropping requirements is safe: body verification still proves every
    // forwarded operation from the remaining executable assumptions.
    if executable.is_empty() {
        return Ok(true);
    }
    let mut authored = Vec::new();
    let mut current = Some(declaration.declaration);
    while let Some(id) = current {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        let view = definitions
            .resolve(id)
            .map_err(|_| TypeTransformError::InvalidContract)?;
        if let Some(part) = view.last()
            && matches!(
                part.kind,
                DefinitionKind::Function | DefinitionKind::Method | DefinitionKind::Impl
            )
        {
            let record = lookup(id).ok_or(TypeTransformError::InvalidContract)?;
            if record.declaration != id {
                return Err(TypeTransformError::InvalidContract);
            }
            let bounds = match (&record.contract, part.kind) {
                (
                    NamedContract::Function(function),
                    DefinitionKind::Function | DefinitionKind::Method,
                ) => &function.bounds,
                (NamedContract::Implementation(implementation), DefinitionKind::Impl) => {
                    &implementation.bounds
                }
                _ => return Err(TypeTransformError::InvalidContract),
            };
            if bounds.len() > MAX_NESTED_RECORDS - authored.len() {
                return Err(TypeTransformError::LimitExceeded);
            }
            authored.extend(substitution.apply_bounds(bounds, definitions, cancel)?);
        }
        current = definitions
            .parent(id)
            .map_err(|_| TypeTransformError::InvalidContract)?;
    }
    let authored = GenericBound::normalize(authored, definitions, cancel)?;
    let executable = TypeSubstitution::default().apply_bounds(executable, definitions, cancel)?;
    let mut remaining = 100_000usize;
    for required in &executable {
        'requirement: for constraint in &required.constraints {
            for available in &authored {
                step(&mut remaining, cancel)?;
                if available.ty != required.ty {
                    continue;
                }
                for candidate in &available.constraints {
                    step(&mut remaining, cancel)?;
                    if candidate.satisfies(constraint) {
                        continue 'requirement;
                    }
                }
            }
            return Ok(false);
        }
    }
    Ok(true)
}

fn step(remaining: &mut usize, cancel: &CancellationToken) -> Result<(), TypeTransformError> {
    cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
    *remaining = remaining
        .checked_sub(1)
        .ok_or(TypeTransformError::LimitExceeded)?;
    Ok(())
}
