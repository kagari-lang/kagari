use super::*;
use kagari_common::collection::CollectionAccess;
use kagari_common::identity::{DefinitionKind, DefinitionPathSegment, ModuleIdentity};

fn definition(module: &str, kind: DefinitionKind) -> DefinitionId {
    DefinitionId {
        module: ModuleIdentity::single_file(module),
        path: vec![DefinitionPathSegment {
            kind,
            name: "Item".into(),
            occurrence: 0,
        }],
    }
}

#[test]
fn collection_access_is_invariant_in_nested_types_and_survives_substitution() {
    use CollectionAccess::{Mutable, ReadOnly};
    let parameter = GenericParameterType {
        owner: definition("collection.kgr", DefinitionKind::Function),
        position: 0,
        name: "T".into(),
    };
    let item = TypeId::Generic(parameter.clone());
    for writable in [
        TypeId::Array(Box::new(item.clone()), Mutable),
        TypeId::Set(Box::new(item.clone()), Mutable),
        TypeId::Map {
            key: Box::new(TypeId::Builtin(BuiltinType::String)),
            value: Box::new(item.clone()),
            access: Mutable,
        },
    ] {
        let readable = writable.read_only_view().unwrap();
        assert_eq!(readable.collection_access(), Some(ReadOnly));
        assert!(writable.can_weaken_to(&readable));
        assert!(!readable.can_weaken_to(&writable));
        assert!(writable.conflicts_with(&readable));
        assert!(readable.conflicts_with(&writable));
        assert_eq!(
            [writable.clone(), readable.clone()]
                .into_iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            2
        );
        let nested_writable = TypeId::Array(Box::new(writable.clone()), Mutable);
        let nested_readable = TypeId::Array(Box::new(readable.clone()), Mutable);
        assert!(!nested_writable.can_weaken_to(&nested_readable));
        assert!(!nested_writable.can_weaken_to(&nested_readable.read_only_view().unwrap()));
        assert!(nested_writable.conflicts_with(&nested_readable));
        assert!(
            TypeId::Tuple(vec![writable]).conflicts_with(&TypeId::Tuple(vec![readable.clone()]))
        );

        let substitution = [(parameter.clone(), TypeId::Builtin(BuiltinType::I32))]
            .into_iter()
            .collect();
        let instantiated = readable.instantiate(&substitution);
        assert_eq!(instantiated.collection_access(), Some(ReadOnly));
        assert!(instantiated.is_concrete());
        assert_eq!(instantiated.map_children(Clone::clone), instantiated);
        assert_eq!(
            instantiated.argument_context(&TypeSubstitution::default(), &[]),
            instantiated
        );
    }
}

#[test]
fn nominal_identity_includes_kind_declaration_and_ordered_type_arguments() {
    let owner = definition("left.kgr", DefinitionKind::Struct);
    let integer = TypeId::Builtin(BuiltinType::I32);
    let boolean = TypeId::Builtin(BuiltinType::Bool);
    let a = NominalType {
        associated_types: Default::default(),
        declaration: owner.clone(),
        arguments: vec![integer.clone(), boolean.clone()],
    };
    let reversed = NominalType {
        associated_types: Default::default(),
        declaration: owner.clone(),
        arguments: vec![boolean, integer],
    };
    let foreign = NominalType {
        associated_types: Default::default(),
        declaration: definition("right.kgr", DefinitionKind::Struct),
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
    assert_eq!(TypeId::Struct(a).display_name(), "Item<i32, bool>");
    assert!(types.iter().all(TypeId::is_concrete));
}

#[test]
fn substitution_preserves_nominal_owners_and_only_replaces_the_selected_binder_layer() {
    let owner = definition("owner.kgr", DefinitionKind::Struct);
    let parameter = GenericParameterType {
        owner: owner.clone(),
        position: 0,
        name: "T".into(),
    };
    let foreign = GenericParameterType {
        owner: definition("other.kgr", DefinitionKind::Function),
        position: 0,
        name: "T".into(),
    };
    let inner = TypeId::Enum(NominalType {
        associated_types: Default::default(),
        declaration: definition("inner.kgr", DefinitionKind::Enum),
        arguments: vec![
            TypeId::Generic(parameter.clone()),
            TypeId::Generic(foreign.clone()),
        ],
    });
    for make in [TypeId::Struct, TypeId::Enum, TypeId::Trait] {
        let template = make(NominalType {
            associated_types: Default::default(),
            declaration: owner.clone(),
            arguments: vec![TypeId::Array(
                Box::new(inner.clone()),
                CollectionAccess::Mutable,
            )],
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
        assert_eq!(once.display_name(), "Item<MutableArray<Item<T, i32>>>");
        let twice = once.instantiate(&substitution);
        assert!(twice.is_concrete());
        assert_eq!(twice.display_name(), "Item<MutableArray<Item<i32, i32>>>");
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
    let owner = definition("owner.kgr", DefinitionKind::Trait);
    let foreign = definition("other.kgr", DefinitionKind::Trait);
    let template = TypeId::Struct(NominalType {
        associated_types: Default::default(),
        declaration: definition("box.kgr", DefinitionKind::Struct),
        arguments: vec![
            TypeId::SelfType(owner.clone()),
            TypeId::Trait(NominalType {
                associated_types: Default::default(),
                declaration: foreign.clone(),
                arguments: vec![TypeId::SelfType(foreign)],
            }),
        ],
    });
    assert_eq!(
        template
            .with_self(&owner, &TypeId::Builtin(BuiltinType::I32))
            .display_name(),
        "Item<i32, Item<Self>>"
    );
}

#[test]
fn substitution_walks_deep_templates_and_copies_deep_replacements_without_recursion() {
    let parameter = GenericParameterType {
        owner: definition("deep.kgr", DefinitionKind::Function),
        position: 0,
        name: "T".into(),
    };
    let mut template = TypeId::Generic(parameter.clone());
    let mut replacement = TypeId::Generic(parameter.clone());
    for _ in 0..10_000 {
        template = TypeId::Array(Box::new(template), CollectionAccess::Mutable);
        replacement = TypeId::Array(Box::new(replacement), CollectionAccess::Mutable);
    }
    let mut substitution: TypeSubstitution =
        [(parameter.clone(), replacement)].into_iter().collect();
    let result = template.instantiate(&substitution);
    // Consume iteratively too: this test exercises substitution, not Rust's
    // recursive derived Clone, equality, or destructor for arbitrary TypeIds.
    fn consume(mut ty: TypeId, depth: usize, parameter: &GenericParameterType) {
        for _ in 0..depth {
            let TypeId::Array(inner, _) = ty else {
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
    let owner = definition("owner.kgr", DefinitionKind::Trait);
    let foreign = definition("foreign.kgr", DefinitionKind::Trait);
    let mut template = TypeId::SelfType(owner.clone());
    let mut replacement = TypeId::SelfType(owner.clone());
    for _ in 0..10_000 {
        template = TypeId::Set(Box::new(template), CollectionAccess::Mutable);
        replacement = TypeId::Set(Box::new(replacement), CollectionAccess::Mutable);
    }
    let result = template.with_self(&owner, &replacement);
    fn consume(mut ty: TypeId, depth: usize, owner: &DefinitionId) {
        for _ in 0..depth {
            let TypeId::Set(inner, _) = ty else {
                panic!("missing set layer")
            };
            ty = *inner;
        }
        assert_eq!(ty, TypeId::SelfType(owner.clone()));
    }
    consume(result, 20_000, &owner);
    consume(template, 10_000, &owner);
    let foreign_result = TypeId::SelfType(foreign.clone()).with_self(&owner, &replacement);
    assert_eq!(foreign_result, TypeId::SelfType(foreign));
    consume(replacement, 10_000, &owner);
}

#[test]
fn semantic_type_predicates_walk_deep_constructed_types_without_recursion() {
    let mut resolved = TypeId::Builtin(BuiltinType::I32);
    let mut unresolved = TypeId::Error;
    let mut comparable = TypeId::Builtin(BuiltinType::I32);
    let mut incomparable = TypeId::Host(definition("host.kgr", DefinitionKind::Struct));
    for _ in 0..10_000 {
        resolved = TypeId::Array(Box::new(resolved), CollectionAccess::Mutable);
        unresolved = TypeId::Array(Box::new(unresolved), CollectionAccess::Mutable);
        comparable = TypeId::Tuple(vec![comparable]);
        incomparable = TypeId::Tuple(vec![incomparable]);
    }
    assert!(resolved.is_concrete());
    assert!(!resolved.is_unresolved());
    assert!(!unresolved.is_concrete());
    assert!(unresolved.is_unresolved());
    assert!(comparable.supports_equality());
    assert!(!incomparable.supports_equality());
    let resolved_name = resolved.display_name();
    assert_eq!(resolved_name.len(), 140_003);
    assert!(resolved_name.starts_with("MutableArray<MutableArray<"));
    assert!(resolved_name.ends_with(">>"));
    let comparable_name = comparable.display_name();
    assert_eq!(comparable_name.len(), 20_003);
    assert!(comparable_name.starts_with("((("));
    assert!(comparable_name.ends_with(")))"));

    for mut ty in [resolved, unresolved] {
        for _ in 0..10_000 {
            let TypeId::Array(inner, _) = ty else {
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
