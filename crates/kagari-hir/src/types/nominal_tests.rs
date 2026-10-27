use super::*;
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId,
};

fn definition(context: &DefinitionContext, module: &str, kind: DefinitionKind) -> DefinitionId {
    let root = context
        .intern_root(&ModuleIdentity::single_file(module))
        .unwrap();
    context.intern_child(root, kind, "Item", 0).unwrap()
}

#[test]
fn native_collection_arguments_remain_invariant_through_substitution() {
    let context = DefinitionContext::new().unwrap();
    let owner = definition(&context, "collection.kgr", DefinitionKind::AssociatedType);
    let parameter = GenericParameterType {
        owner,
        position: 0,
        name: "T".into(),
    };
    let application = |arguments| {
        TypeId::NativeObject(NominalType {
            declaration: owner,
            arguments,
            associated_types: Default::default(),
        })
    };
    for arguments in [
        vec![TypeId::Generic(parameter.clone())],
        vec![
            TypeId::Builtin(BuiltinType::String),
            TypeId::Generic(parameter.clone()),
        ],
    ] {
        let template = application(arguments);
        let integer = template.instantiate(
            &[(parameter.clone(), TypeId::Builtin(BuiltinType::I32))]
                .into_iter()
                .collect(),
        );
        let boolean = template.instantiate(
            &[(parameter.clone(), TypeId::Builtin(BuiltinType::Bool))]
                .into_iter()
                .collect(),
        );
        assert!(integer.conflicts_with(&boolean));
        assert!(boolean.conflicts_with(&integer));
        assert!(!integer.can_weaken_to(&boolean));
        assert_eq!(
            [integer.clone(), boolean.clone()]
                .into_iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            2
        );
        assert!(
            application(vec![integer.clone()]).conflicts_with(&application(vec![boolean.clone()]))
        );
        assert!(TypeId::Tuple(vec![integer.clone()]).conflicts_with(&TypeId::Tuple(vec![boolean])));
        assert!(integer.is_concrete());
        assert_eq!(integer.map_children(Clone::clone), integer);
        assert_eq!(
            integer.argument_context(&TypeSubstitution::default(), &[]),
            integer
        );
    }
}

#[test]
fn nominal_identity_includes_kind_declaration_and_ordered_type_arguments() {
    let context = DefinitionContext::new().unwrap();
    let owner = definition(&context, "left.kgr", DefinitionKind::Struct);
    let integer = TypeId::Builtin(BuiltinType::I32);
    let boolean = TypeId::Builtin(BuiltinType::Bool);
    let a = NominalType {
        associated_types: Default::default(),
        declaration: owner,
        arguments: vec![integer.clone(), boolean.clone()],
    };
    let reversed = NominalType {
        associated_types: Default::default(),
        declaration: owner,
        arguments: vec![boolean, integer],
    };
    let foreign = NominalType {
        associated_types: Default::default(),
        declaration: definition(&context, "right.kgr", DefinitionKind::Struct),
        arguments: a.arguments.clone(),
    };
    let bare = NominalType {
        associated_types: Default::default(),
        declaration: owner,
        arguments: Vec::new(),
    };
    let types = [
        TypeId::Struct(a.clone()),
        TypeId::Enum(a.clone()),
        TypeId::Trait(a.clone()),
        TypeId::Struct(reversed),
        TypeId::Struct(foreign),
        TypeId::Struct(bare),
    ];
    let set = types
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(set.len(), types.len());
    assert_eq!(
        TypeId::Struct(a).display_name(&context.snapshot()),
        "Item<i32, bool>"
    );
    assert!(types.iter().all(TypeId::is_concrete));
}

#[test]
fn substitution_preserves_nominal_owners_and_only_replaces_the_selected_binder_layer() {
    let context = DefinitionContext::new().unwrap();
    let owner = definition(&context, "owner.kgr", DefinitionKind::Struct);
    let parameter = GenericParameterType {
        owner,
        position: 0,
        name: "T".into(),
    };
    let foreign = GenericParameterType {
        owner: definition(&context, "other.kgr", DefinitionKind::Function),
        position: 0,
        name: "T".into(),
    };
    let inner = TypeId::Enum(NominalType {
        associated_types: Default::default(),
        declaration: definition(&context, "inner.kgr", DefinitionKind::Enum),
        arguments: vec![
            TypeId::Generic(parameter.clone()),
            TypeId::Generic(foreign.clone()),
        ],
    });
    for make in [
        TypeId::NativeObject,
        TypeId::Struct,
        TypeId::Enum,
        TypeId::Trait,
    ] {
        let template = make(NominalType {
            associated_types: Default::default(),
            declaration: owner,
            arguments: vec![TypeId::Array(Box::new(inner.clone()))],
        });
        assert!(!template.is_concrete());
        assert!(!template.is_unresolved());
        let substitution = [
            (parameter.clone(), TypeId::Generic(foreign.clone())),
            (foreign.clone(), TypeId::Builtin(BuiltinType::I32)),
        ]
        .into_iter()
        .collect();
        let once = template.instantiate(&substitution);
        assert!(!once.is_concrete());
        assert_eq!(
            once.display_name(&context.snapshot()),
            "Item<[Item<T, i32>]>"
        );
        let twice = once.instantiate(&substitution);
        assert!(twice.is_concrete());
        assert_eq!(
            twice.display_name(&context.snapshot()),
            "Item<[Item<i32, i32>]>"
        );
        let substituted_error =
            template.instantiate(&[(parameter.clone(), TypeId::Error)].into_iter().collect());
        assert!(substituted_error.is_unresolved());
        // The error in the first member does not hide the independent mismatch
        // between the foreign binder and i32 in the second member.
        assert!(substituted_error.conflicts_with(&twice));
    }
}

#[test]
fn self_substitution_reaches_nested_nominal_arguments_without_replacing_foreign_self() {
    let context = DefinitionContext::new().unwrap();
    let owner = definition(&context, "owner.kgr", DefinitionKind::Trait);
    let foreign = definition(&context, "other.kgr", DefinitionKind::Trait);
    let template = TypeId::Struct(NominalType {
        associated_types: Default::default(),
        declaration: definition(&context, "box.kgr", DefinitionKind::Struct),
        arguments: vec![
            TypeId::SelfType(owner),
            TypeId::Trait(NominalType {
                associated_types: Default::default(),
                declaration: foreign,
                arguments: vec![TypeId::SelfType(foreign)],
            }),
        ],
    });
    assert_eq!(
        template
            .with_self(&owner, &TypeId::Builtin(BuiltinType::I32))
            .display_name(&context.snapshot()),
        "Item<i32, Item<Self>>"
    );
}

#[test]
fn substitution_walks_deep_templates_and_copies_deep_replacements_without_recursion() {
    let context = DefinitionContext::new().unwrap();
    let parameter = GenericParameterType {
        owner: definition(&context, "deep.kgr", DefinitionKind::Function),
        position: 0,
        name: "T".into(),
    };
    let mut template = TypeId::Generic(parameter.clone());
    let mut replacement = TypeId::Generic(parameter.clone());
    for _ in 0..10_000 {
        template = TypeId::Array(Box::new(template));
        replacement = TypeId::Array(Box::new(replacement));
    }
    let mut substitution: TypeSubstitution =
        [(parameter.clone(), replacement)].into_iter().collect();
    let result = template.instantiate(&substitution);
    // Consume iteratively too: this test exercises substitution, not Rust's
    // recursive derived Clone, equality, or destructor for arbitrary TypeIds.
    fn consume(mut ty: TypeId, depth: usize, parameter: &GenericParameterType) {
        for _ in 0..depth {
            let TypeId::Array(inner) = ty else {
                panic!("missing array layer")
            };
            ty = *inner;
        }
        assert_eq!(ty, TypeId::Generic(parameter.clone()));
    }
    consume(result, 20_000, &parameter);
    consume(template, 10_000, &parameter);
    for (_, replacement) in substitution.drain() {
        consume(replacement, 10_000, &parameter);
    }
}

#[test]
fn self_substitution_copies_deep_replacements_once_and_preserves_foreign_owners() {
    let context = DefinitionContext::new().unwrap();
    let owner = definition(&context, "owner.kgr", DefinitionKind::Trait);
    let foreign = definition(&context, "foreign.kgr", DefinitionKind::Trait);
    let mut template = TypeId::SelfType(owner);
    let mut replacement = TypeId::SelfType(owner);
    for _ in 0..10_000 {
        template = TypeId::NativeObject(NominalType {
            declaration: owner,
            arguments: vec![template],
            associated_types: Default::default(),
        });
        replacement = TypeId::NativeObject(NominalType {
            declaration: owner,
            arguments: vec![replacement],
            associated_types: Default::default(),
        });
    }
    let result = template.with_self(&owner, &replacement);
    fn consume(mut ty: TypeId, depth: usize, owner: &DefinitionId) {
        for _ in 0..depth {
            let TypeId::NativeObject(mut inner) = ty else {
                panic!("missing nominal layer")
            };
            ty = inner.arguments.pop().unwrap();
        }
        assert_eq!(ty, TypeId::SelfType(*owner));
    }
    consume(result, 20_000, &owner);
    consume(template, 10_000, &owner);
    let foreign_result = TypeId::SelfType(foreign).with_self(&owner, &replacement);
    assert_eq!(foreign_result, TypeId::SelfType(foreign));
    consume(replacement, 10_000, &owner);
}

#[test]
fn semantic_type_predicates_walk_deep_constructed_types_without_recursion() {
    let context = DefinitionContext::new().unwrap();
    let mut resolved = TypeId::Builtin(BuiltinType::I32);
    let mut unresolved = TypeId::Error;
    let mut comparable = TypeId::Builtin(BuiltinType::I32);
    let mut incomparable = TypeId::Host(definition(&context, "host.kgr", DefinitionKind::Struct));
    for _ in 0..10_000 {
        resolved = TypeId::Array(Box::new(resolved));
        unresolved = TypeId::Array(Box::new(unresolved));
        comparable = TypeId::Tuple(vec![comparable]);
        incomparable = TypeId::Tuple(vec![incomparable]);
    }
    assert!(resolved.is_concrete());
    assert!(!resolved.is_unresolved());
    assert!(!unresolved.is_concrete());
    assert!(unresolved.is_unresolved());
    let resolved_name = resolved.display_name(&context.snapshot());
    assert_eq!(resolved_name.len(), 20_003);
    assert!(resolved_name.starts_with("[["));
    assert!(resolved_name.ends_with("]]"));
    let comparable_name = comparable.display_name(&context.snapshot());
    assert_eq!(comparable_name.len(), 20_003);
    assert!(comparable_name.starts_with("((("));
    assert!(comparable_name.ends_with(")))"));

    for mut ty in [resolved, unresolved] {
        for _ in 0..10_000 {
            let TypeId::Array(inner) = ty else {
                panic!("missing array layer")
            };
            ty = *inner;
        }
    }
    for mut ty in [comparable, incomparable] {
        for _ in 0..10_000 {
            let TypeId::Tuple(mut items) = ty else {
                panic!("missing tuple layer")
            };
            ty = items.pop().unwrap();
        }
    }
}
