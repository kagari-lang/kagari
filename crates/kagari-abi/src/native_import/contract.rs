//! Trusted storage and callback guards applied to carried signatures, including
//! declaration templates. No source-facing declaration is reconstructed here.
use crate::{
    callable::EngineNativeBinding,
    native_import::{NativeSignature, signature},
    scalar::BuiltinType,
    standard::{
        StandardIntrinsic,
        surface::{StandardEnum, StandardTypeConstraint},
        traits::StandardTrait,
    },
    types::{AbiType, ConstraintAbi, GenericBoundAbi, NominalAbiType},
};
use kagari_common::collection::CollectionAccess;

mod callbacks;
mod defaults;
mod protocol;

pub fn binding_signature_valid(
    binding: EngineNativeBinding,
    signature: &NativeSignature,
    bounds: &[GenericBoundAbi],
) -> bool {
    if signature
        .params
        .iter()
        .chain([&signature.result])
        .any(|ty| !ty.within_wire_limits())
    {
        return false;
    }
    match binding {
        EngineNativeBinding::TraitDefault(method) => defaults::valid(method, signature, bounds),
        EngineNativeBinding::Protocol(method) => protocol::valid(method, signature, bounds),
        EngineNativeBinding::Intrinsic(operation) => {
            if signature::validate(binding, signature).is_some() {
                return true;
            }
            if signature::discarded_storage(binding, signature).is_some() {
                return true;
            }
            if matches!(
                operation,
                StandardIntrinsic::MathMin
                    | StandardIntrinsic::MathMax
                    | StandardIntrinsic::MathClamp
                    | StandardIntrinsic::MathAbs
            ) {
                let required = if operation == StandardIntrinsic::MathAbs {
                    StandardTypeConstraint::SignedNumber
                } else {
                    StandardTypeConstraint::OrderedNumber
                };
                let params_valid = match (operation, signature.params.as_slice()) {
                    (StandardIntrinsic::MathAbs, [value]) => value == &signature.result,
                    (StandardIntrinsic::MathMin | StandardIntrinsic::MathMax, [a, b]) => {
                        a == b && a == &signature.result
                    }
                    (StandardIntrinsic::MathClamp, [a, b, c]) => {
                        a == b && b == c && a == &signature.result
                    }
                    _ => false,
                };
                return params_valid
                    && bounds.iter().any(|bound| {
                        bound.ty == signature.result
                            && bound
                                .constraints
                                .contains(&ConstraintAbi::Standard(required))
                    });
            }
            callbacks::valid(operation, signature, bounds)
        }
        _ => signature::validate(binding, signature).is_some(),
    }
}

fn builtin(kind: BuiltinType) -> AbiType {
    AbiType::Builtin(kind)
}
fn enumeration(kind: StandardEnum, args: Vec<AbiType>) -> AbiType {
    AbiType::StandardEnum { kind, args }
}
fn option(item: &AbiType) -> AbiType {
    enumeration(StandardEnum::Option, vec![item.clone()])
}
fn result(item: &AbiType, error: &AbiType) -> AbiType {
    enumeration(StandardEnum::Result, vec![item.clone(), error.clone()])
}
fn callback(ty: &AbiType, inputs: &[AbiType], output: &AbiType) -> bool {
    matches!(ty, AbiType::Function { params, result } if params == inputs && result.as_ref() == output)
}
fn bound<'a>(
    bounds: &'a [GenericBoundAbi],
    ty: &AbiType,
    kind: StandardTrait,
) -> Option<&'a NominalAbiType> {
    bounds
        .iter()
        .filter(|bound| &bound.ty == ty)
        .flat_map(|bound| &bound.constraints)
        .find_map(|constraint| match constraint {
            ConstraintAbi::Trait(interface)
                if StandardTrait::from_id(&interface.declaration) == Some(kind) =>
            {
                Some(interface)
            }
            _ => None,
        })
}
fn member<'a>(interface: &'a NominalAbiType, name: &str) -> Option<&'a AbiType> {
    interface.associated_types.iter().find_map(|(id, ty)| {
        (id.path.last().is_some_and(|p| p.name == name)
            && id.module == interface.declaration.module
            && id.path[..id.path.len() - 1] == interface.declaration.path)
            .then_some(ty)
    })
}
fn iterable_item<'a>(bounds: &'a [GenericBoundAbi], source: &AbiType) -> Option<&'a AbiType> {
    member(bound(bounds, source, StandardTrait::Iterable)?, "Item")
}
fn collection_item(ty: &AbiType) -> Option<AbiType> {
    match ty {
        AbiType::Array(item, _)
        | AbiType::Set(item, _)
        | AbiType::Iter(item)
        | AbiType::Range(item, _) => Some(item.as_ref().clone()),
        AbiType::Map { key, value, .. } => Some(AbiType::Tuple(vec![
            key.as_ref().clone(),
            value.as_ref().clone(),
        ])),
        AbiType::Builtin(BuiltinType::String) => Some(ty.clone()),
        _ => None,
    }
}
fn mutable_array(ty: &AbiType) -> Option<&AbiType> {
    match ty {
        AbiType::Array(item, CollectionAccess::Mutable) => Some(item),
        _ => None,
    }
}

fn readonly_collection(ty: &AbiType, kind: StandardTrait, args: &[AbiType]) -> bool {
    matches!(ty, AbiType::Trait(interface) if StandardTrait::from_id(&interface.declaration) == Some(kind) && interface.arguments == args && interface.associated_types.is_empty())
}
