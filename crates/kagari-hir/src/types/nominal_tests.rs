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
        assert!(!substituted_error.conflicts_with(&twice));
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
