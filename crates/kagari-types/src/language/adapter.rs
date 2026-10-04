//! Closed implicit protocol signature checks; executable adapter selection is external.
use crate::{
    callable::Signature,
    conversion::lossless_from,
    declaration::{module::ModuleDecl, requirement::NativeCallableRequirement},
    language::{Protocol, binding},
    scalar::BuiltinType,
    ty::Ty,
};
use kagari_common::identity::{DefinitionKind, associated_type_id};
pub fn adapter_contract(required: &NativeCallableRequirement) -> Option<(Protocol, Signature)> {
    let kind = Protocol::from_id(&required.interface.declaration)?;
    if kind == Protocol::From {
        let [input] = required.interface.arguments.as_slice() else {
            return None;
        };
        let eligible = input == &required.receiver
            || matches!((input, &required.receiver), (Ty::Builtin(source), Ty::Builtin(target))
                if lossless_from(*source, *target));
        return (eligible
            && required.member == ModuleDecl::method_id(&required.interface.declaration, "from")
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
        return (required.member == ModuleDecl::method_id(&required.interface.declaration, "call")
            && required.arguments.is_empty()
            && required.interface.arguments == [packed.clone()]
            && required.interface.associated_types.len() == 1
            && required.interface.associated_types.get(&associated_type_id(
                &required.interface.declaration,
                "Output",
            )) == Some(result.as_ref()))
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
                .get(&associated_type_id(&required.interface.declaration, "Iter"))
                .cloned()
                .unwrap_or_else(|| required.receiver.clone()),
            1,
        ),
        Protocol::Iterator if matches!(required.receiver, Ty::Trait(_)) => (
            "next",
            binding::option(
                required
                    .interface
                    .associated_types
                    .get(&associated_type_id(&required.interface.declaration, "Item"))?
                    .clone(),
            ),
            1,
        ),
        Protocol::PartialEq => ("eq", Ty::Builtin(BuiltinType::Bool), 2),
        Protocol::Hash => ("hash", Ty::Builtin(BuiltinType::I64), 1),
        Protocol::Debug => ("debug", Ty::Builtin(BuiltinType::String), 1),
        Protocol::Display => ("display", Ty::Builtin(BuiltinType::String), 1),
        Protocol::Ord => ("cmp", binding::ordering(), 2),
        Protocol::PartialOrd => ("partial_cmp", binding::option(binding::ordering()), 2),
        _ => return None,
    };
    let mut owner = required.member.clone();
    let method = owner.path.pop()?;
    if owner != required.interface.declaration
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
                    *member != associated_type_id(&required.interface.declaration, "Item")
                        && (*member != associated_type_id(&required.interface.declaration, "Iter")
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
