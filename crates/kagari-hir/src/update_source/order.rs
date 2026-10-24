//! Deterministic collection order, repeated after semantic identity correspondence.

use crate::update_source::SourceEvidence;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMappingError, check_cancel},
        table::DefinitionTable,
    },
};
use kagari_types::ty::{Ty, substitution::TypeTransformError};
use std::cmp::Ordering;

fn type_error(error: TypeTransformError) -> DefinitionMappingError {
    match error {
        TypeTransformError::Cancelled => DefinitionMappingError::Cancelled,
        TypeTransformError::LimitExceeded => DefinitionMappingError::LimitExceeded,
        TypeTransformError::InvalidContract => DefinitionMappingError::InvalidContract,
    }
}

fn sort<T>(
    values: &mut [T],
    mut compare: impl FnMut(&T, &T) -> Result<Ordering, DefinitionMappingError>,
) -> Result<(), DefinitionMappingError> {
    let mut failure = None;
    values.sort_by(|left, right| match compare(left, right) {
        Ok(order) => order,
        Err(error) => {
            failure = Some(error);
            Ordering::Equal
        }
    });
    failure.map_or(Ok(()), Err)
}

fn compare_types(
    left: &[Ty],
    right: &[Ty],
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<Ordering, DefinitionMappingError> {
    for (left, right) in left.iter().zip(right) {
        let order = left
            .canonical_cmp(right, definitions, cancel)
            .map_err(type_error)?;
        if !order.is_eq() {
            return Ok(order);
        }
    }
    Ok(left.len().cmp(&right.len()))
}

impl SourceEvidence {
    /// Order inventory sets by their resolved identities, not allocation order.
    /// Call again after declaration correspondence changes implementation owners.
    /// Facts inside a body retain their deterministic HIR traversal order.
    pub fn canonical_order(
        &mut self,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        sort(&mut self.bodies, |a, b| {
            check_cancel(cancel)?;
            Ok(definitions.compare(a.declaration, b.declaration)?)
        })?;
        for root in &mut self.roots {
            sort(&mut root.members, |a, b| {
                check_cancel(cancel)?;
                Ok(definitions.compare(*a, *b)?)
            })?;
        }
        sort(&mut self.roots, |a, b| {
            check_cancel(cancel)?;
            let order = definitions
                .compare(a.declaration, b.declaration)?
                .then(a.kind.cmp(&b.kind));
            if !order.is_eq() {
                return Ok(order);
            }
            let left: Vec<_> = a
                .receiver
                .iter()
                .cloned()
                .chain(a.interface.iter().cloned().map(Ty::Trait))
                .collect();
            let right: Vec<_> = b
                .receiver
                .iter()
                .cloned()
                .chain(b.interface.iter().cloned().map(Ty::Trait))
                .collect();
            compare_types(&left, &right, definitions, cancel)
        })?;
        sort(&mut self.selections, |a, b| {
            check_cancel(cancel)?;
            for (a, b) in a.targets.iter().zip(&b.targets) {
                let order = definitions.compare(*a, *b)?;
                if !order.is_eq() {
                    return Ok(order);
                }
            }
            let order = a
                .targets
                .len()
                .cmp(&b.targets.len())
                .then(a.kind.cmp(&b.kind));
            if !order.is_eq() {
                return Ok(order);
            }
            compare_types(&a.types, &b.types, definitions, cancel)
        })?;
        for body in &mut self.bodies {
            for bound in &mut body.bounds {
                sort(&mut bound.constraints, |a, b| {
                    Ty::Trait(a.clone())
                        .canonical_cmp(&Ty::Trait(b.clone()), definitions, cancel)
                        .map_err(type_error)
                })?;
            }
            sort(&mut body.bounds, |a, b| {
                a.ty.canonical_cmp(&b.ty, definitions, cancel)
                    .map_err(type_error)
            })?;
        }
        Ok(())
    }
}
