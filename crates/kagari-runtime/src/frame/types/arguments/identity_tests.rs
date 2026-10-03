use super::*;
use kagari_abi::{callable::generic::GenericBody, scalar::BuiltinType, types::GenericParameterAbi};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, map::DefinitionContext,
};

fn owner(module: &str) -> DefinitionPath {
    DefinitionPath {
        module: ModuleIdentity::single_file(module),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Function,
            name: "same".into(),
            occurrence: 0,
        }],
    }
}

fn binder(definitions: &DefinitionContext, module: &str) -> GenericParameterAbi<DefinitionId> {
    GenericParameterAbi {
        owner: definitions.intern(&owner(module)).unwrap(),
        position: 0,
    }
}

fn argument(kind: BuiltinType) -> TypeArgument {
    TypeArgument {
        ty: AbiType::Builtin(kind),
        definitions: DefinitionContext::new().unwrap().snapshot(),
        origin: None,
    }
}

#[test]
fn scoped_frame_binders_preserve_parent_scope_and_survive_context_owner_drop() {
    let definitions = DefinitionContext::new().unwrap();
    let parent = Rc::new(
        TypeEnvironment::new(
            &definitions,
            vec![binder(&definitions, "parent.kgr")],
            vec![argument(BuiltinType::I32)],
        )
        .unwrap(),
    );
    let mut child = TypeEnvironment::new(
        &definitions,
        vec![binder(&definitions, "child.kgr")],
        vec![argument(BuiltinType::String)],
    )
    .unwrap();
    child.include(Some(parent.clone())).unwrap();
    let body = GenericBody {
        parameters: vec![
            binder(&definitions, "child.kgr"),
            binder(&definitions, "parent.kgr"),
        ],
        bounds: vec![],
    };
    assert!(child.matches(&body));
    let mut reversed = body.clone();
    reversed.parameters.reverse();
    assert!(!child.matches(&reversed));
    let expression = AbiType::Tuple(
        body.parameters
            .iter()
            .map(GenericParameterAbi::as_type)
            .collect(),
    );
    let expected = AbiType::Tuple(vec![
        AbiType::Builtin(BuiltinType::String),
        AbiType::Builtin(BuiltinType::I32),
    ]);
    assert_eq!(child.resolve(&expression).unwrap(), expected);
    let types_only = child.types_only();
    drop(child);
    drop(parent);
    let foreign = definitions.intern(&owner("foreign.kgr")).unwrap();
    drop(definitions);
    assert_eq!(types_only.resolve(&expression).unwrap(), expected);
    assert!(types_only.argument(&foreign, 0).is_none());
}

#[test]
fn environments_reject_duplicate_binders_and_foreign_context_parents() {
    let definitions = DefinitionContext::new().unwrap();
    let parent = Rc::new(
        TypeEnvironment::new(
            &definitions,
            vec![binder(&definitions, "owner.kgr")],
            vec![argument(BuiltinType::I32)],
        )
        .unwrap(),
    );
    let mut duplicate = TypeEnvironment::new(
        &definitions,
        vec![binder(&definitions, "owner.kgr")],
        vec![argument(BuiltinType::String)],
    )
    .unwrap();
    assert!(duplicate.include(Some(parent.clone())).is_err());
    let foreign = DefinitionContext::new().unwrap();
    let mut child = TypeEnvironment::new(
        &foreign,
        vec![binder(&foreign, "other.kgr")],
        vec![argument(BuiltinType::String)],
    )
    .unwrap();
    assert!(child.include(Some(parent)).is_err());
}
