//! Closed language protocol contracts eligible for generated callable adapters.
use crate::{
    declaration::ModuleDecl,
    language::Protocol,
    native_import::{NativeSignature, callables::NativeCallableRequirement},
    scalar::BuiltinType,
    standard::surface::StandardEnum,
    types::AbiType,
};
use kagari_common::identity::{DefinitionKind, associated_type_id};

pub fn adapter_contract(
    required: &NativeCallableRequirement,
) -> Option<(Protocol, NativeSignature)> {
    let kind = Protocol::from_id(&required.interface.declaration)?;
    if kind == Protocol::Fn {
        let AbiType::Function { params, result } = &required.receiver else {
            return None;
        };
        let packed = if params.is_empty() {
            AbiType::Builtin(BuiltinType::Unit)
        } else {
            AbiType::Tuple(params.clone())
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
                NativeSignature {
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
        Protocol::Iterator if matches!(required.receiver, AbiType::Trait(_)) => (
            "next",
            AbiType::StandardEnum {
                kind: StandardEnum::Option,
                args: vec![
                    required
                        .interface
                        .associated_types
                        .get(&associated_type_id(&required.interface.declaration, "Item"))?
                        .clone(),
                ],
            },
            1,
        ),
        Protocol::PartialEq => ("eq", AbiType::Builtin(BuiltinType::Bool), 2),
        Protocol::Hash => ("hash", AbiType::Builtin(BuiltinType::I64), 1),
        Protocol::Debug => ("debug", AbiType::Builtin(BuiltinType::String), 1),
        Protocol::Display => ("display", AbiType::Builtin(BuiltinType::String), 1),
        Protocol::Ord => (
            "cmp",
            AbiType::StandardEnum {
                kind: StandardEnum::Ordering,
                args: vec![],
            },
            2,
        ),
        Protocol::PartialOrd => (
            "partial_cmp",
            AbiType::StandardEnum {
                kind: StandardEnum::Option,
                args: vec![AbiType::StandardEnum {
                    kind: StandardEnum::Ordering,
                    args: vec![],
                }],
            },
            2,
        ),
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
                            || (!matches!(required.receiver, AbiType::Trait(_))
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
        NativeSignature {
            params: vec![required.receiver.clone(); count],
            result,
        },
    ))
}

/// Applied associated outputs are part of a generated iterator adapter's identity.
pub fn adapter_arguments(required: &NativeCallableRequirement) -> Vec<AbiType> {
    let mut arguments = vec![required.receiver.clone()];
    if Protocol::from_id(&required.interface.declaration)
        .is_some_and(|kind| kind.iteration() || kind == Protocol::Fn)
    {
        arguments.push(AbiType::Trait(required.interface.clone()));
    }
    arguments
}
