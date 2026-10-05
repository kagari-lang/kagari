use kagari_common::identity::DefinitionKind;
use kagari_common::identity::{DefinitionPath, DefinitionPathSegment, ModuleIdentity};
use kagari_types::{
    declaration::{TypeDef, native::NativeTypeConstructor},
    range::RangeKind,
    visibility::Visibility,
};
use {
    crate::types::{PublicItem, verify},
    kagari_types::{
        declaration::{FieldDef, TypeDefKind, VariantDef},
        scalar::BuiltinType,
        ty::{GenericParam, Ty},
    },
};

fn declaration(kind: NativeTypeConstructor) -> (ModuleIdentity, TypeDef) {
    let module = ModuleIdentity::single_file("native-contract.kgr");
    let owner = DefinitionPath {
        module: module.clone(),
        path: vec![DefinitionPathSegment {
            kind: kind.declaration_kind(),
            name: "Native".into(),
            occurrence: 0,
        }],
    };
    let generic_params: Vec<_> = (0..kind.arity())
        .map(|position| GenericParam {
            owner: owner.clone(),
            position,
        })
        .collect();
    let variants = Vec::new();
    (
        module,
        TypeDef {
            name: "Native".into(),
            kind: TypeDefKind::Native(kind),
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
    ];
    for kind in constructors {
        let (module, ty) = declaration(kind);
        let valid = |ty: TypeDef| {
            verify::validate(&[PublicItem::Type(ty)], &module, &Default::default()).is_ok()
        };
        assert!(valid(ty.clone()), "{kind:?}");
        let mut wrong = ty.clone();
        wrong.fields.push(FieldDef {
            visibility: Visibility::Public,
            name: "injected".into(),
            ty: Ty::Builtin(BuiltinType::I32),
            mutable: false,
        });
        assert!(
            !valid(wrong),
            "{kind:?}: fields cannot replace native storage"
        );
        let mut wrong = ty.clone();
        wrong.variants.push(VariantDef {
            reports_failure: false,
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
            wrong.generic_params.push(GenericParam {
                owner: DefinitionPath {
                    module: module.clone(),
                    path: vec![],
                },
                position: 0,
            });
            assert!(!valid(wrong), "{kind:?}: unexpected generic argument");
        }
        for index in 0..ty.variants.len() {
            let mut wrong = ty.clone();
            wrong.variants[index].payload = vec![Ty::Builtin(BuiltinType::Bool)];
            assert!(!valid(wrong), "{kind:?}: payload slot {index}");
        }
    }
}

#[test]
fn ordinary_enum_layouts_validate_payloads_and_reporting_facts() {
    use crate::layout::{EnumLayout, EnumVariantLayout, enum_abi_matches};
    use kagari_types::declaration::module::ModuleDecl;
    let module = ModuleIdentity::single_file("ordinary-enum.kgr");
    let owner = DefinitionPath {
        module: module.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Enum,
            name: "Outcome".into(),
            occurrence: 0,
        }],
    };
    let parameter = GenericParam {
        owner: owner.clone(),
        position: 0,
    };
    let declaration = TypeDef {
        name: "Outcome".into(),
        kind: TypeDefKind::Enum,
        generic_params: vec![parameter.clone()],
        bounds: vec![],
        fields: vec![],
        variants: vec![
            VariantDef {
                reports_failure: true,
                name: "Failure".into(),
                payload: vec![parameter.as_type()],
            },
            VariantDef {
                reports_failure: false,
                name: "Success".into(),
                payload: vec![],
            },
        ],
    };
    let layout = EnumLayout {
        declaration: owner.clone(),
        arguments: vec![Ty::Builtin(BuiltinType::String)],
        variants: vec![
            EnumVariantLayout {
                reports_failure: true,
                declaration: ModuleDecl::variant_id(&owner, "Failure"),
                payload: vec![Ty::Builtin(BuiltinType::String)],
            },
            EnumVariantLayout {
                reports_failure: false,
                declaration: ModuleDecl::variant_id(&owner, "Success"),
                payload: vec![],
            },
        ],
    };
    let items = vec![PublicItem::Type(declaration)];
    assert!(verify::validate(&items, &module, &Default::default()).is_ok());
    assert!(
        enum_abi_matches(
            std::slice::from_ref(&layout),
            &module,
            &items,
            &Default::default()
        )
        .unwrap()
    );
    for case in 0..4 {
        let mut wrong = layout.clone();
        match case {
            0 => wrong.arguments.clear(),
            1 => wrong.variants[0].payload.clear(),
            2 => wrong.variants[0].payload[0] = Ty::Builtin(BuiltinType::Bool),
            _ => wrong.variants[0].reports_failure = false,
        }
        assert!(
            !enum_abi_matches(&[wrong], &module, &items, &Default::default()).unwrap(),
            "case {case}"
        );
    }
}
