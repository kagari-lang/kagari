//! The standard provider's offline contracts; no source parser or executable handlers.
use kagari_abi::{
    effects::EffectSet,
    native_import::NativeSignature,
    provider::{NativeBindingKey, NativeContract, NativeParameterAccess},
    scalar::BuiltinType,
    types::AbiType,
};
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity, PackageId},
};

pub const PROVIDER: u64 = 0x6b61676172697374;

pub fn contracts() -> Vec<(&'static str, NativeContract)> {
    let binder = DefinitionId {
        module: ModuleIdentity {
            package: PackageId("kagari-std-provider".into()),
            path: vec!["array".into()],
        },
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Function,
            name: "array".into(),
            occurrence: 0,
        }],
    };
    let item = AbiType::Parameter {
        owner: binder.clone(),
        position: 0,
    };
    let array = AbiType::Array(Box::new(item.clone()), CollectionAccess::Mutable);
    let unit = AbiType::Builtin(BuiltinType::Unit);
    let size = AbiType::Builtin(BuiltinType::USize);
    let make = |entry, params, result, effects, parameter_access| NativeContract {
        key: NativeBindingKey {
            provider: PROVIDER,
            entry,
        },
        version: 1,
        binder: binder.clone(),
        generic_count: 1,
        host: None,
        parameter_access,
        signature: NativeSignature { params, result },
        effects,
    };
    vec![
        (
            "array_new",
            make(1, vec![], array.clone(), EffectSet::allocation(), vec![]),
        ),
        (
            "array_len",
            make(
                2,
                vec![array.clone()],
                size.clone(),
                EffectSet::aggregate_read(),
                vec![NativeParameterAccess::Read],
            ),
        ),
        (
            "array_push",
            make(
                3,
                vec![array.clone(), item.clone()],
                unit,
                EffectSet::aggregate_write().union(EffectSet::allocation()),
                vec![NativeParameterAccess::Write, NativeParameterAccess::Value],
            ),
        ),
        (
            "array_from_fn",
            make(
                4,
                vec![
                    size.clone(),
                    AbiType::Function {
                        params: vec![size],
                        result: Box::new(item),
                    },
                ],
                array,
                EffectSet::runtime_call()
                    .union(EffectSet::allocation())
                    .union(EffectSet::aggregate_write()),
                vec![NativeParameterAccess::Value, NativeParameterAccess::Value],
            ),
        ),
    ]
}
