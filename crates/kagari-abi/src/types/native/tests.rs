use super::*;
use crate::{
    operations::StandardEnumOp,
    representation::ValueType,
    scalar::BuiltinType,
    types::{
        AbiType, FieldAbi, GenericParameterAbi, PublicAbiItem, TypeAbiKind, VariantAbi, verify,
    },
};
use kagari_common::identity::{DefinitionId, DefinitionPathSegment, ModuleIdentity};

fn declaration(kind: NativeTypeConstructor) -> (ModuleIdentity, TypeAbi) {
    let module = ModuleIdentity::single_file("native-contract.kgr");
    let owner = DefinitionId {
        module: module.clone(),
        path: vec![DefinitionPathSegment {
            kind: kind.declaration_kind(),
            name: "Native".into(),
            occurrence: 0,
        }],
    };
    let generic_params: Vec<_> = (0..kind.arity())
        .map(|position| GenericParameterAbi {
            owner: owner.clone(),
            position,
        })
        .collect();
    let variants = match kind {
        NativeTypeConstructor::Enum(kind) => kind
            .variants()
            .iter()
            .enumerate()
            .map(|(index, variant)| VariantAbi {
                name: format!("variant{index}"),
                payload: variant
                    .payload()
                    .into_iter()
                    .map(|slot| generic_params[slot].as_type())
                    .collect(),
            })
            .collect(),
        _ => Vec::new(),
    };
    (
        module,
        TypeAbi {
            name: "Native".into(),
            kind: TypeAbiKind::Native(kind),
            generic_params,
            bounds: Vec::new(),
            fields: Vec::new(),
            variants,
        },
    )
}

#[test]
fn native_type_templates_validate_arity_owner_and_physical_shape() {
    let constructors = [
        NativeTypeConstructor::String,
        NativeTypeConstructor::Array,
        NativeTypeConstructor::Map,
        NativeTypeConstructor::Set,
        NativeTypeConstructor::Iter,
        NativeTypeConstructor::Range(RangeKind::Exclusive),
        NativeTypeConstructor::Range(RangeKind::Inclusive),
        NativeTypeConstructor::Range(RangeKind::From),
        NativeTypeConstructor::Range(RangeKind::To),
        NativeTypeConstructor::Range(RangeKind::ToInclusive),
        NativeTypeConstructor::Range(RangeKind::Full),
        NativeTypeConstructor::Enum(StandardEnum::Bound),
        NativeTypeConstructor::Enum(StandardEnum::ParseError),
        NativeTypeConstructor::Enum(StandardEnum::TryFromIntError),
        NativeTypeConstructor::Enum(StandardEnum::Infallible),
        NativeTypeConstructor::Enum(StandardEnum::Option),
        NativeTypeConstructor::Enum(StandardEnum::Result),
        NativeTypeConstructor::Enum(StandardEnum::Ordering),
    ];
    for kind in constructors {
        let (module, ty) = declaration(kind);
        let valid = |ty: TypeAbi| {
            verify::validate(&[PublicAbiItem::Type(ty)], &module, &Default::default()).is_ok()
        };
        assert!(valid(ty.clone()), "{kind:?}");
        let mut wrong = ty.clone();
        wrong.fields.push(FieldAbi {
            name: "injected".into(),
            ty: AbiType::Builtin(BuiltinType::I32),
            mutable: false,
        });
        assert!(
            !valid(wrong),
            "{kind:?}: fields cannot replace native storage"
        );
        let mut wrong = ty.clone();
        wrong.variants.push(VariantAbi {
            name: "extra".into(),
            payload: Vec::new(),
        });
        assert!(!valid(wrong), "{kind:?}: variant count");
        if let Some(parameter) = ty.generic_params.first() {
            let mut wrong = ty.clone();
            wrong.generic_params.pop();
            assert!(!valid(wrong), "{kind:?}: missing generic argument");
            let mut wrong = ty.clone();
            wrong.generic_params[0].owner.path[0].kind = DefinitionKind::Struct;
            assert!(!valid(wrong), "{kind:?}: binder owner");
            let mut wrong = ty.clone();
            wrong.generic_params.push(parameter.clone());
            assert!(!valid(wrong), "{kind:?}: extra generic argument");
        } else {
            let mut wrong = ty.clone();
            wrong.generic_params.push(GenericParameterAbi {
                owner: DefinitionId {
                    module: module.clone(),
                    path: vec![],
                },
                position: 0,
            });
            assert!(!valid(wrong), "{kind:?}: unexpected generic argument");
        }
        for index in 0..ty.variants.len() {
            let mut wrong = ty.clone();
            wrong.variants[index].payload = vec![AbiType::Builtin(BuiltinType::Bool)];
            assert!(!valid(wrong), "{kind:?}: payload slot {index}");
        }
    }
}

#[test]
fn enum_operations_follow_wire_tags_and_result_payload_slots() {
    let result = AbiType::StandardEnum {
        kind: StandardEnum::Result,
        args: vec![
            AbiType::Builtin(BuiltinType::I32),
            AbiType::Builtin(BuiltinType::String),
        ],
    };
    assert_eq!(
        StandardEnumOp::Make(0).contract(&result),
        Some((Some(ValueType::I32), ValueType::HeapObject))
    );
    assert_eq!(
        StandardEnumOp::Make(1).contract(&result),
        Some((Some(ValueType::Str), ValueType::HeapObject))
    );
    assert_eq!(
        StandardEnumOp::Read(1).contract(&result),
        Some((Some(ValueType::HeapObject), ValueType::Str))
    );
    assert!(StandardEnumOp::Test(2).contract(&result).is_none());
    for kind in [
        StandardEnum::Bound,
        StandardEnum::ParseError,
        StandardEnum::TryFromIntError,
        StandardEnum::Infallible,
        StandardEnum::Option,
        StandardEnum::Result,
        StandardEnum::Ordering,
    ] {
        for (index, variant) in kind.variants().iter().enumerate() {
            assert_eq!(variant.kind(), kind);
            assert_eq!(variant.index(), index);
            assert!(variant.payload().is_none_or(|slot| slot < kind.arity()));
        }
    }
    let empty = AbiType::StandardEnum {
        kind: StandardEnum::Infallible,
        args: vec![],
    };
    assert!(StandardEnumOp::Make(0).contract(&empty).is_none());
    let wrong_arity = AbiType::StandardEnum {
        kind: StandardEnum::Result,
        args: vec![],
    };
    assert!(StandardEnumOp::Make(0).contract(&wrong_arity).is_none());
}
