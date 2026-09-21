use super::*;
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
fn nominal_identity_includes_kind_declaration_and_ordered_type_arguments() {
    let owner = definition("left.kgr", DefinitionKind::Struct);
    let integer = TypeId::Builtin(BuiltinType::I32);
    let boolean = TypeId::Builtin(BuiltinType::Bool);
    let a = NominalType {
        declaration: owner.clone(),
        arguments: vec![integer.clone(), boolean.clone()],
    };
    let reversed = NominalType {
        declaration: owner.clone(),
        arguments: vec![boolean, integer],
    };
    let foreign = NominalType {
        declaration: definition("right.kgr", DefinitionKind::Struct),
        arguments: a.arguments.clone(),
    };
    let bare = NominalType {
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
        declaration: definition("inner.kgr", DefinitionKind::Enum),
        arguments: vec![
            TypeId::Generic(parameter.clone()),
            TypeId::Generic(foreign.clone()),
        ],
    });
    for make in [TypeId::Struct, TypeId::Enum, TypeId::Trait] {
        let template = make(NominalType {
            declaration: owner.clone(),
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
        assert_eq!(once.display_name(), "Item<[Item<T, i32>]>");
        let twice = once.instantiate(&substitution);
        assert!(twice.is_concrete());
        assert_eq!(twice.display_name(), "Item<[Item<i32, i32>]>");
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
        declaration: definition("box.kgr", DefinitionKind::Struct),
        arguments: vec![
            TypeId::SelfType(owner.clone()),
            TypeId::Trait(NominalType {
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
        template = TypeId::Array(Box::new(template));
        replacement = TypeId::Array(Box::new(replacement));
    }
    let substitution = [(parameter.clone(), replacement)].into_iter().collect();
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
    for (_, replacement) in substitution {
        consume(replacement, 10_000, &parameter);
    }
}
