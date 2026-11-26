use crate::layout::{EnumLayout, EnumVariantLayout, enum_abi_matches, validate_enum_layouts};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_types::{
    declaration::{
        TypeDef,
        native::{NativeStorageLayout, NativeTypeConstructor},
    },
    language::binding,
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

fn declaration(
    context: &DefinitionContext,
    kind: NativeTypeConstructor,
) -> (ModuleIdentity, TypeDef) {
    let path = kind.declaration();
    let owner = context.intern(&path).unwrap();
    let generic_params: Vec<_> = (0..kind.arity())
        .map(|position| GenericParam { owner, position })
        .collect();
    let variants = Vec::new();
    (
        path.module,
        TypeDef {
            name: path.path[0].name.clone(),
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
        NativeTypeConstructor::Range(RangeKind::Exclusive),
        NativeTypeConstructor::Range(RangeKind::Inclusive),
        NativeTypeConstructor::Range(RangeKind::From),
        NativeTypeConstructor::Range(RangeKind::To),
        NativeTypeConstructor::Range(RangeKind::ToInclusive),
        NativeTypeConstructor::Range(RangeKind::Full),
    ];
    for kind in constructors {
        let context = DefinitionContext::new().unwrap();
        let (module, ty) = declaration(&context, kind);
        let valid = |ty: TypeDef| {
            verify::validate(
                &[PublicItem::Type(ty)],
                &module,
                &Default::default(),
                &context.snapshot(),
            )
            .is_ok()
        };
        assert!(valid(ty.clone()), "{kind:?}");
        if kind == NativeTypeConstructor::String {
            let other = ModuleIdentity::single_file("other.kgr");
            let root = context.intern_root(&other).unwrap();
            context
                .intern_child(root, kind.declaration_kind(), &ty.name, 0)
                .unwrap();
            assert!(
                verify::validate(
                    &[PublicItem::Type(ty.clone())],
                    &other,
                    &Default::default(),
                    &context.snapshot()
                )
                .is_err(),
                "correct shape, foreign owner"
            );
            let mut ordinary = ty.clone();
            ordinary.kind = TypeDefKind::NativeStorage(NativeStorageLayout::Opaque);
            assert!(
                !valid(ordinary),
                "canonical definition cannot claim opaque storage"
            );
        }
        let mut wrong = ty.clone();
        wrong.fields.push(FieldDef {
            host_policy: None,
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
            wrong.generic_params[0].owner = context
                .intern_child(
                    context.intern_root(&module).unwrap(),
                    DefinitionKind::Struct,
                    &ty.name,
                    0,
                )
                .unwrap();
            assert!(!valid(wrong), "{kind:?}: binder owner");
            let mut wrong = ty.clone();
            wrong.generic_params.push(parameter.clone());
            assert!(!valid(wrong), "{kind:?}: extra generic argument");
        } else {
            let mut wrong = ty.clone();
            wrong.generic_params.push(GenericParam {
                owner: context.intern_root(&module).unwrap(),
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
fn result_layouts_validate_payloads_and_reporting_facts() {
    let path = binding::result_declaration();
    let module = path.module.clone();
    let context = DefinitionContext::new().unwrap();
    let owner = context.intern(&path).unwrap();
    let parameters = [0, 1].map(|position| GenericParam { owner, position });
    let arguments = [
        Ty::Builtin(BuiltinType::I32),
        Ty::Builtin(BuiltinType::String),
    ];
    let declaration = TypeDef {
        name: "Result".into(),
        kind: TypeDefKind::Enum,
        generic_params: parameters.to_vec(),
        bounds: vec![],
        fields: vec![],
        variants: ["Ok", "Err"]
            .into_iter()
            .enumerate()
            .map(|(slot, name)| VariantDef {
                reports_failure: slot == 1,
                name: name.into(),
                payload: vec![parameters[slot].as_type()],
            })
            .collect(),
    };
    let layout = EnumLayout {
        declaration: owner,
        arguments: arguments.to_vec(),
        variants: ["Ok", "Err"]
            .into_iter()
            .enumerate()
            .map(|(slot, name)| EnumVariantLayout {
                reports_failure: slot == 1,
                declaration: context
                    .intern_child(owner, DefinitionKind::Variant, name, 0)
                    .unwrap(),
                payload: vec![arguments[slot].clone()],
            })
            .collect(),
    };
    // Identical member names, arguments and payloads under a different package.
    let other = context
        .intern_child(
            context
                .intern_root(&ModuleIdentity::single_file("lookalike"))
                .unwrap(),
            DefinitionKind::Enum,
            "Result",
            0,
        )
        .unwrap();
    let mut lookalike = layout.clone();
    lookalike.declaration = other;
    for (variant, name) in lookalike.variants.iter_mut().zip(["Ok", "Err"]) {
        variant.declaration = context
            .intern_child(other, DefinitionKind::Variant, name, 0)
            .unwrap();
    }
    let items = vec![PublicItem::Type(declaration)];
    let table = context.snapshot();
    let cancel = Default::default();
    assert!(verify::validate(&items, &module, &cancel, &table).is_ok());
    let matches = |layout: &EnumLayout| {
        enum_abi_matches(
            std::slice::from_ref(layout),
            &module,
            &items,
            &cancel,
            &table,
        )
        .unwrap()
    };
    let valid = |layout: &EnumLayout| {
        validate_enum_layouts(std::slice::from_ref(layout), &[], &cancel, &table).is_ok()
    };
    assert!(matches(&layout) && valid(&layout));
    assert!(!valid(&lookalike));
    lookalike.variants[1].reports_failure = false;
    assert!(valid(&lookalike));
    for case in 0..5 {
        let mut wrong = layout.clone();
        match case {
            0 => wrong.arguments.clear(),
            1 => wrong.variants[1].payload.clear(),
            2 => wrong.variants[1].payload[0] = arguments[0].clone(),
            3 => wrong.variants[1].reports_failure = false,
            _ => wrong.variants.swap(0, 1),
        }
        assert!(!matches(&wrong), "ABI case {case}");
        assert!(!valid(&wrong), "standalone layout case {case}");
    }
}
