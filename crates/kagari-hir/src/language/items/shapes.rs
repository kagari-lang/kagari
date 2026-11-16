//! Independent language-required shapes, checked against actual semantic declarations.

use crate::{
    aggregates::{AggregateCatalog, traits::TraitSignature},
    types::semantic::{lower_nominal_type, lower_type},
};
use kagari_common::identity::{
    DefinitionKind,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    declaration::conversion::ConversionAdapter,
    language::{Protocol, binding},
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
    let result_valid = |result: &DefinitionId| {
        aggregates
            .enumerations()
            .any(|item| item.id == *result && item.generic_params.len() == 2)
    };
    match relationship {
        ConversionAdapter::Forward { result, .. } => result_valid(result),
        ConversionAdapter::Reverse {
            origin,
            method,
            error,
            result,
        } => {
            let Some(forward) = aggregates.trait_(origin) else {
                return false;
            };
            let [parameter] = forward.generic_params.as_slice() else {
                return false;
            };
            let [selected] = forward.methods.as_slice() else {
                return false;
            };
            let input = Ty::Parameter {
                owner: parameter.owner,
                position: parameter.position,
            };
            let this = Ty::SelfType(*origin);
            let output = match (error, result) {
                (None, None) => this,
                (Some((_, member)), Some(result))
                    if result_valid(result) && forward.associated_types.contains_key(member) =>
                {
                    Ty::Enum(NominalTy {
                        declaration: *result,
                        arguments: vec![
                            this.clone(),
                            Ty::Projection {
                                receiver: Box::new(this),
                                interface: Box::new(NominalTy {
                                    declaration: *origin,
                                    arguments: vec![input.clone()],
                                    associated_types: BTreeMap::new(),
                                }),
                                member: *member,
                                arguments: vec![],
                            },
                        ],
                        associated_types: BTreeMap::new(),
                    })
                }
                _ => return false,
            };
            selected.id == *method
                && selected.generic_params == forward.generic_params
                && selected.bounds.values().all(Vec::is_empty)
                && forward.bounds.values().all(Vec::is_empty)
                && selected.params.len() == 1
                && selected.params[0].name != "self"
                && lower_type(&selected.params[0].ty) == input
                && lower_type(&selected.return_type) == output
        }
    }
}

/// Extension defaults are ordinary checked declarations. Only required language
/// operations are fixed here; no producer-supplied copy can validate its own shape.
pub(super) fn valid(
    actual: &TraitSignature,
    kind: Protocol,
    definitions: &DefinitionTable,
) -> bool {
    valid_inner(actual, kind, definitions).unwrap_or(false)
}

fn valid_inner(
    actual: &TraitSignature,
    kind: Protocol,
    definitions: &DefinitionTable,
) -> Option<bool> {
    let arity = usize::from(
        kind.binary_operator()
            || matches!(
                kind,
                Protocol::Index | Protocol::Fn | Protocol::From | Protocol::FromResidual
            ),
    );
    if actual.generic_params.len() != arity
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
    let parameter = || shape.interface.arguments[0].clone();
    let member_names: &[&str] = match kind {
        Protocol::Iterator => &["Item"],
        Protocol::Iterable => &["Item", "Iter"],
        Protocol::Try => &["Output", "Residual"],
        kind if kind.operator() => &["Output"],
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
    let parents: &[Protocol] = match kind {
        Protocol::Eq | Protocol::PartialOrd => &[Protocol::PartialEq],
        Protocol::Ord => &[Protocol::Eq, Protocol::PartialOrd],
        Protocol::Try => &[Protocol::FromResidual],
        _ => &[],
    };
    let mut expected_parents = Vec::new();
    for parent in parents {
        expected_parents.push(NominalTy {
            declaration: parent.definition(definitions)?,
            arguments: if kind == Protocol::Try {
                vec![shape.projection("Residual")?]
            } else {
                vec![]
            },
            associated_types: BTreeMap::new(),
        });
    }
    if actual
        .supertraits
        .iter()
        .map(lower_nominal_type)
        .collect::<Vec<_>>()
        != expected_parents
    {
        return Some(false);
    }
    let ordering = || {
        Some(shape.enumeration(
            definitions.lookup(&binding::ordering_declaration())?,
            vec![],
        ))
    };
    let option = |ty| {
        Some(shape.enumeration(
            definitions.lookup(&binding::option_declaration())?,
            vec![ty],
        ))
    };
    let (name, params, result) = match kind {
        Protocol::Eq => return Some(actual.methods.is_empty()),
        Protocol::PartialEq => (
            "eq",
            vec![("self", this.clone()), ("arg1", this)],
            Ty::Builtin(BuiltinType::Bool),
        ),
        Protocol::PartialOrd => (
            "partial_cmp",
            vec![("self", this.clone()), ("arg1", this)],
            option(ordering()?)?,
        ),
        Protocol::Ord => (
            "cmp",
            vec![("self", this.clone()), ("arg1", this)],
            ordering()?,
        ),
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
        Protocol::From => ("from", vec![("value", parameter())], this),
        Protocol::FromResidual => ("from_residual", vec![("residual", parameter())], this),
        Protocol::Try => {
            let output = shape.projection("Output")?;
            let residual = shape.projection("Residual")?;
            let branch = shape.enumeration(
                definitions.lookup(&binding::control_flow_declaration())?,
                vec![residual, output.clone()],
            );
            return Some(
                actual.methods.len() == 2
                    && shape.method("from_output", &[("output", output)], &this)
                    && shape.method("branch", &[("self", this)], &branch),
            );
        }
        _ => {
            let name = match kind {
                Protocol::Add => "add",
                Protocol::Sub => "sub",
                Protocol::Mul => "mul",
                Protocol::Div => "div",
                Protocol::Rem => "rem",
                Protocol::BitAnd => "bitand",
                Protocol::BitOr => "bitor",
                Protocol::BitXor => "bitxor",
                Protocol::Shl => "shl",
                Protocol::Shr => "shr",
                Protocol::Neg => "neg",
                Protocol::Not => "not",
                Protocol::Index => "index",
                Protocol::Fn => "call",
                _ => return None,
            };
            let mut params = vec![("self", this)];
            if arity == 1 {
                params.push(("arg1", parameter()));
            }
            (name, params, shape.projection("Output")?)
        }
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
