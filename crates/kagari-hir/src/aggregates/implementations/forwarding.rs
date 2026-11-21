//! Adapt checked HIR headers to the shared library forwarding admission rule.

use super::ImplementationSignature;
use crate::types::{
    NominalType, TypeId,
    semantic::{lower_nominal_type, lower_type},
};
use kagari_common::identity::table::DefinitionTable;
use kagari_types::{
    declaration::conversion::library_forwarding_header_valid,
    ty::{GenericBound, GenericParam},
};

pub(super) fn canonical_header(
    definitions: &DefinitionTable,
    implementation: &ImplementationSignature,
) -> bool {
    let unresolved = |interface: &NominalType| {
        interface
            .arguments
            .iter()
            .chain(interface.associated_types.values())
            .any(TypeId::is_unresolved)
    };
    if implementation.for_type.is_unresolved() || unresolved(&implementation.trait_type) {
        return false;
    }
    let mut bounds = Vec::new();
    for (target, constraints) in &implementation.bounds {
        if constraints.is_empty() {
            continue;
        }
        if target.is_unresolved() || constraints.iter().any(unresolved) {
            return false;
        }
        bounds.push(GenericBound {
            ty: lower_type(target),
            constraints: constraints.iter().map(lower_nominal_type).collect(),
        });
    }
    let parameters = implementation
        .generic_params
        .iter()
        .map(|p| GenericParam {
            owner: p.owner,
            position: p.position,
        })
        .collect::<Vec<_>>();
    library_forwarding_header_valid(
        definitions,
        implementation.id,
        &parameters,
        &lower_type(&implementation.for_type),
        &lower_nominal_type(&implementation.trait_type),
        &bounds,
    )
}
