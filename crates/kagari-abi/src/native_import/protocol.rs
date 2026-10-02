//! Closed language protocol contracts eligible for generated callable adapters.
use crate::{
    language::Protocol,
    native_import::{NativeSignature, callables::NativeCallableRequirement},
    scalar::BuiltinType,
    standard::surface::StandardEnum,
    types::AbiType,
};
use kagari_common::identity::DefinitionKind;

pub fn adapter_contract(
    required: &NativeCallableRequirement,
) -> Option<(Protocol, NativeSignature)> {
    let kind = Protocol::from_id(&required.interface.declaration)?;
    let (member, result, count) = match kind {
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
        || !required.interface.associated_types.is_empty()
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
