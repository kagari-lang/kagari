//! Independent language-required shapes, checked against actual semantic declarations.

use super::projection;
use crate::aggregates::{AggregateCatalog, traits::TraitSignature};
use kagari_common::identity::table::DefinitionTable;
use kagari_types::language::{Protocol, comparison, requirements};

/// Relationship IDs must name supplied declarations with the required forward
/// signature. Merely interning matching paths in a definition table is not proof.
pub(super) fn conversion_targets_valid(
    actual: &TraitSignature,
    aggregates: &AggregateCatalog,
) -> bool {
    let Some(relationship) = &actual.conversion_adapter else {
        return true;
    };
    aggregates
        .enumerations()
        .any(|item| item.id == relationship.result && item.generic_params.len() == 2)
}

/// Extension defaults are ordinary checked declarations. Only required language
/// operations are fixed here; no producer-supplied copy can validate its own shape.
pub(super) fn valid(
    actual: &TraitSignature,
    kind: Protocol,
    definitions: &DefinitionTable,
) -> bool {
    kind.definition(definitions) == Some(actual.id)
        && projection::simple_trait(actual, definitions).is_some_and(|record| {
            if kind.comparison() {
                comparison::trait_shape_valid(actual.id, &record, definitions)
            } else {
                requirements::trait_shape_valid(actual.id, &record, definitions)
            }
        })
}
