//! Independent language-required shapes, checked against actual semantic declarations.

use super::projection;
use crate::{
    aggregates::{AggregateCatalog, traits::TraitSignature},
    types::semantic::{lower_nominal_type, lower_type},
};
use kagari_common::identity::{
    DefinitionKind,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    language::{Protocol, binding, comparison, requirements},
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::collections::BTreeMap;

struct Shape<'a> {
    actual: &'a TraitSignature,
    definitions: &'a DefinitionTable,
    interface: NominalTy,
}

impl Shape<'_> {
    fn associated(&self, name: &str) -> Option<DefinitionId> {
        self.definitions
            .lookup_child(self.actual.id, DefinitionKind::AssociatedType, name, 0)
    }

    fn projection(&self, name: &str) -> Option<Ty> {
        Some(Ty::Projection {
            receiver: Box::new(Ty::SelfType(self.actual.id)),
            interface: Box::new(self.interface.clone()),
            member: self.associated(name)?,
            arguments: vec![],
        })
    }

    fn enumeration(&self, declaration: DefinitionId, arguments: Vec<Ty>) -> Ty {
        Ty::Enum(NominalTy {
            declaration,
            arguments,
            associated_types: BTreeMap::new(),
        })
    }

    fn method(&self, name: &str, params: &[(&str, Ty)], result: &Ty) -> bool {
        let Some(method) = self
            .actual
            .methods
            .iter()
            .find(|method| method.name == name)
        else {
            return false;
        };
        method.default.is_none()
            && method.policy.override_allowed
            && method.policy.receiver_access.is_none()
            && method.generic_params == self.actual.generic_params
            && method.bounds.values().all(Vec::is_empty)
            && method.params.len() == params.len()
            && method
                .params
                .iter()
                .zip(params)
                .all(|(actual, (name, ty))| {
                    actual.name == *name
                        && !actual.writeability.is_var()
                        && lower_type(&actual.ty) == *ty
                })
            && lower_type(&method.return_type) == *result
    }
}

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
    if kind.comparison() {
        return kind.definition(definitions) == Some(actual.id)
            && projection::simple_trait(actual, definitions).is_some_and(|record| {
                comparison::trait_shape_valid(actual.id, &record, definitions)
            });
    }
    if kind.source_operation() {
        return kind.definition(definitions) == Some(actual.id)
            && projection::simple_trait(actual, definitions).is_some_and(|record| {
                requirements::trait_shape_valid(actual.id, &record, definitions)
            });
    }
    valid_inner(actual, kind, definitions).unwrap_or(false)
}

fn valid_inner(
    actual: &TraitSignature,
    kind: Protocol,
    definitions: &DefinitionTable,
) -> Option<bool> {
    if !actual.generic_params.is_empty()
        || !actual.bounds.values().all(Vec::is_empty)
        || !actual.associated_consts.is_empty()
        || !actual.associated_type_parameters.is_empty()
        || actual.conversion_adapter.is_some()
    {
        return Some(false);
    }
    let interface = NominalTy {
        declaration: actual.id,
        arguments: actual
            .generic_params
            .iter()
            .map(|parameter| Ty::Parameter {
                owner: parameter.owner,
                position: parameter.position,
            })
            .collect(),
        associated_types: BTreeMap::new(),
    };
    let shape = Shape {
        actual,
        definitions,
        interface,
    };
    let this = Ty::SelfType(actual.id);

    let member_names: &[&str] = match kind {
        Protocol::Iterator => &["Item"],
        Protocol::Iterable => &["Item", "Iter"],

        _ => &[],
    };
    if actual.associated_types.len() != member_names.len() {
        return Some(false);
    }
    for name in member_names {
        let bounds = actual.associated_types.get(&shape.associated(name)?)?;
        if kind == Protocol::Iterable && *name == "Iter" {
            let iterator = Protocol::Iterator.definition(definitions)?;
            let item =
                definitions.lookup_child(iterator, DefinitionKind::AssociatedType, "Item", 0)?;
            let expected = NominalTy {
                declaration: iterator,
                arguments: vec![],
                associated_types: [(item, shape.projection("Item")?)].into(),
            };
            if bounds.iter().map(lower_nominal_type).collect::<Vec<_>>() != [expected] {
                return Some(false);
            }
        } else if !bounds.is_empty() {
            return Some(false);
        }
    }
    if !actual.supertraits.is_empty() {
        return None;
    }
    let option = |ty| {
        Some(shape.enumeration(
            definitions.lookup(&binding::option_declaration())?,
            vec![ty],
        ))
    };
    let (name, params, result) = match kind {
        Protocol::Hash => ("hash", vec![("self", this)], Ty::Builtin(BuiltinType::I64)),
        Protocol::Debug => (
            "debug",
            vec![("self", this)],
            Ty::Builtin(BuiltinType::String),
        ),
        Protocol::Display => (
            "display",
            vec![("self", this)],
            Ty::Builtin(BuiltinType::String),
        ),
        Protocol::Iterator => (
            "next",
            vec![("self", this)],
            option(shape.projection("Item")?)?,
        ),
        Protocol::Iterable => ("iter", vec![("self", this)], shape.projection("Iter")?),
        _ => return None,
    };
    let member_count_valid = if kind == Protocol::Iterable {
        actual
            .methods
            .iter()
            .all(|method| method.name == "iter" || method.default.is_some())
    } else {
        actual.methods.len() == 1
    };
    Some(member_count_valid && shape.method(name, &params, &result))
}
