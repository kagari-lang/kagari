//! Closed implicit protocol signature checks; executable adapter selection is external.
use crate::{
    callable::Signature,
    conversion::lossless_from,
    declaration::requirement::NativeCallableRequirement,
    language::{Protocol, binding},
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use kagari_common::identity::{DefinitionKind, table::DefinitionTable};

pub fn adapter_contract(
    required: &NativeCallableRequirement,
    definitions: &DefinitionTable,
) -> Option<(Protocol, Signature)> {
    let member_id = |name| {
        definitions.lookup_child(
            required.interface.declaration,
            DefinitionKind::AssociatedType,
            name,
            0,
        )
    };
    let enum_type = |id, arguments| {
        Ty::Enum(NominalTy {
            declaration: id,
            arguments,
            associated_types: Default::default(),
        })
    };
    let kind = Protocol::from_definition(&required.interface.declaration, definitions)?;
    if kind == Protocol::From {
        let [input] = required.interface.arguments.as_slice() else {
            return None;
        };
        let eligible = input == &required.receiver
            || matches!((input, &required.receiver), (Ty::Builtin(source), Ty::Builtin(target))
                if lossless_from(*source, *target));
        return (eligible
            && Some(required.member)
                == definitions.lookup_child(
                    required.interface.declaration,
                    DefinitionKind::Method,
                    "from",
                    0,
                )
            && required.arguments.is_empty()
            && required.interface.associated_types.is_empty())
        .then(|| {
            (
                kind,
                Signature {
                    params: vec![input.clone()],
                    result: required.receiver.clone(),
                },
            )
        });
    }
    if kind == Protocol::Fn {
        let Ty::Function { params, result } = &required.receiver else {
            return None;
        };
        let packed = if params.is_empty() {
            Ty::Builtin(BuiltinType::Unit)
        } else {
            Ty::Tuple(params.clone())
        };
        return (Some(required.member)
            == definitions.lookup_child(
                required.interface.declaration,
                DefinitionKind::Method,
                "call",
                0,
            )
            && required.arguments.is_empty()
            && required.interface.arguments == [packed.clone()]
            && required.interface.associated_types.len() == 1
            && required
                .interface
                .associated_types
                .get(&member_id("Output")?)
                == Some(result.as_ref()))
        .then(|| {
            (
                kind,
                Signature {
                    params: vec![required.receiver.clone(), packed],
                    result: result.as_ref().clone(),
                },
            )
        });
    }
    let (member, result, count) = match kind {
        Protocol::Iterable => (
            "iter",
            required
                .interface
                .associated_types
                .get(&member_id("Iter")?)
                .cloned()
                .unwrap_or_else(|| required.receiver.clone()),
            1,
        ),
        Protocol::Iterator if matches!(required.receiver, Ty::Trait(_)) => (
            "next",
            enum_type(
                definitions.lookup(&binding::option_declaration())?,
                vec![
                    required
                        .interface
                        .associated_types
                        .get(&member_id("Item")?)?
                        .clone(),
                ],
            ),
            1,
        ),
        Protocol::PartialEq => ("eq", Ty::Builtin(BuiltinType::Bool), 2),
        Protocol::Hash => ("hash", Ty::Builtin(BuiltinType::I64), 1),
        Protocol::Debug => ("debug", Ty::Builtin(BuiltinType::String), 1),
        Protocol::Display => ("display", Ty::Builtin(BuiltinType::String), 1),
        Protocol::Ord => (
            "cmp",
            enum_type(
                definitions.lookup(&binding::ordering_declaration())?,
                vec![],
            ),
            2,
        ),
        Protocol::PartialOrd => (
            "partial_cmp",
            enum_type(
                definitions.lookup(&binding::option_declaration())?,
                vec![enum_type(
                    definitions.lookup(&binding::ordering_declaration())?,
                    vec![],
                )],
            ),
            2,
        ),
        _ => return None,
    };
    let method = definitions.resolve(required.member).ok()?.last()?;
    if definitions.parent(required.member).ok()? != Some(required.interface.declaration)
        || method.kind != DefinitionKind::Method
        || method.name != member
        || method.occurrence != 0
        || !required.arguments.is_empty()
        || !required.interface.arguments.is_empty()
        || if kind.iteration() {
            required
                .interface
                .associated_types
                .iter()
                .any(|(member, ty)| {
                    Some(*member) != member_id("Item")
                        && (Some(*member) != member_id("Iter")
                            || (!matches!(required.receiver, Ty::Trait(_))
                                && *ty != required.receiver))
                })
        } else {
            !required.interface.associated_types.is_empty()
        }
    {
        return None;
    }
    Some((
        kind,
        Signature {
            params: vec![required.receiver.clone(); count],
            result,
        },
    ))
}
