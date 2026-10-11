//! Canonical native bound merging uses exact scoped identities, never slot order.
use crate::ty::{Constraint, GenericBound, Ty, substitution::TypeTransformError};
use kagari_common::{
    cancellation::CancellationToken,
    identity::table::{DefinitionId, DefinitionTable},
};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    mem,
};

impl GenericBound<DefinitionId> {
    /// Merge repeated targets and constraints in canonical semantic order.
    /// Validates identity scope and type bounds without granting type-check evidence.
    /// Cancellation or invalid input returns an error before publishing any result.
    pub fn normalize(
        bounds: impl IntoIterator<Item = Self>,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Vec<Self>, TypeTransformError> {
        let mut merged = BTreeMap::<Ty<DefinitionId>, BTreeSet<Constraint<DefinitionId>>>::new();
        for bound in bounds {
            bound.ty.canonical_cmp(&bound.ty, definitions, cancel)?;
            let constraints = merged.entry(bound.ty).or_default();
            for constraint in bound.constraints {
                cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
                constraints.insert(constraint);
            }
        }
        let mut result = merged
            .into_iter()
            .map(|(ty, constraints)| Self {
                ty,
                constraints: constraints.into_iter().collect(),
            })
            .collect::<Vec<_>>();
        sort(&mut result, &|left, right| {
            left.ty.canonical_cmp(&right.ty, definitions, cancel)
        })?;
        for bound in &mut result {
            bound.constraints =
                Constraint::normalize(mem::take(&mut bound.constraints), definitions, cancel)?;
        }
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        Ok(result)
    }
}

impl Constraint<DefinitionId> {
    /// Deduplicate constraints and order them by exact identity in the supplied scope.
    /// Associated outputs and ordinary generic bounds share this normalization.
    pub fn normalize(
        constraints: impl IntoIterator<Item = Self>,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Vec<Self>, TypeTransformError> {
        let mut unique = BTreeSet::new();
        for constraint in constraints {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if let Self::Trait(interface) = &constraint {
                let ty = Ty::Trait(interface.clone());
                ty.canonical_cmp(&ty, definitions, cancel)?;
            }
            unique.insert(constraint);
        }
        let mut result = unique.into_iter().collect::<Vec<_>>();
        sort(&mut result, &|left, right| {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            match (left, right) {
                (Self::Standard(left), Self::Standard(right)) => Ok(left.cmp(right)),
                (Self::Standard(_), Self::Trait(_)) => Ok(Ordering::Less),
                (Self::Trait(_), Self::Standard(_)) => Ok(Ordering::Greater),
                (Self::Trait(left), Self::Trait(right)) => Ty::Trait(left.clone()).canonical_cmp(
                    &Ty::Trait(right.clone()),
                    definitions,
                    cancel,
                ),
            }
        })?;
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        Ok(result)
    }
}

// Fallible merge sorting avoids using numeric-ID order or swallowing cancellation
// inside a comparator. The owning result is discarded if a comparison fails.
fn sort<T: Clone>(
    values: &mut [T],
    compare: &impl Fn(&T, &T) -> Result<Ordering, TypeTransformError>,
) -> Result<(), TypeTransformError> {
    if values.len() < 2 {
        return Ok(());
    }
    let middle = values.len() / 2;
    let (left, right) = values.split_at_mut(middle);
    sort(left, compare)?;
    sort(right, compare)?;
    let mut merged = Vec::with_capacity(left.len() + right.len());
    let (mut l, mut r) = (0, 0);
    while l < left.len() && r < right.len() {
        if compare(&left[l], &right[r])? != Ordering::Greater {
            merged.push(left[l].clone());
            l += 1;
        } else {
            merged.push(right[r].clone());
            r += 1;
        }
    }
    merged.extend_from_slice(&left[l..]);
    merged.extend_from_slice(&right[r..]);
    values.clone_from_slice(&merged);
    Ok(())
}
