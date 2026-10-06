use super::*;
use crate::{
    frame::types::{EnvironmentRecord, TypeEnvironment, operations::OperationBindings},
    layout_fixtures,
    module::{EnumVariantRef, StructLayoutRef},
    native::{callable::StoredCallable, cursor::NativeCursor, storage_type::StorageType},
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, map::DefinitionContext,
};
use kagari_contract::callable::generic::GenericBody;
use kagari_types::{
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy},
};
use std::thread;

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

fn binder(definitions: &DefinitionContext, module: &str) -> GenericParam<DefinitionId> {
    GenericParam {
        owner: definitions.intern(&owner(module)).unwrap(),
        position: 0,
    }
}

fn argument(kind: BuiltinType) -> TypeArgument {
    TypeArgument {
        data: Arc::new(TypeArgumentData {
            ty: Ty::Builtin(kind),
            definitions: DefinitionContext::new().unwrap().snapshot(),
            origin: None,
            parameters: OnceLock::new(),
            variants: OnceLock::new(),
        }),
    }
}

#[test]
fn scoped_frame_binders_preserve_parent_scope_and_survive_context_owner_drop() {
    let definitions = DefinitionContext::new().unwrap();
    let parent = Arc::new(
        TypeBindings::new(
            &definitions,
            vec![binder(&definitions, "parent.kgr")],
            vec![argument(BuiltinType::I32)],
        )
        .unwrap(),
    );
    let mut child = TypeBindings::new(
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
    let expression = Ty::Tuple(body.parameters.iter().map(GenericParam::as_type).collect());
    let expected = Ty::Tuple(vec![
        Ty::Builtin(BuiltinType::String),
        Ty::Builtin(BuiltinType::I32),
    ]);
    assert_eq!(child.resolve(&expression).unwrap(), expected);
    let types_only = child.clone();
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
    let parent = Arc::new(
        TypeBindings::new(
            &definitions,
            vec![binder(&definitions, "owner.kgr")],
            vec![argument(BuiltinType::I32)],
        )
        .unwrap(),
    );
    let mut duplicate = TypeBindings::new(
        &definitions,
        vec![binder(&definitions, "owner.kgr")],
        vec![argument(BuiltinType::String)],
    )
    .unwrap();
    assert!(duplicate.include(Some(parent.clone())).is_err());
    let foreign = DefinitionContext::new().unwrap();
    let mut child = TypeBindings::new(
        &foreign,
        vec![binder(&foreign, "other.kgr")],
        vec![argument(BuiltinType::String)],
    )
    .unwrap();
    assert!(child.include(Some(parent)).is_err());
}

#[test]
fn nominal_type_origins_keep_bindings_without_retaining_execution_parents() {
    let mut runtime = Runtime::default();
    let layout = layout_fixtures::layout(&mut runtime, "Record", &[]);
    let nominal = Ty::Struct(NominalTy {
        declaration: layout.layout().declaration,
        arguments: vec![],
        associated_types: Default::default(),
    });
    let parameter = binder(runtime.definition_context(), "parent-scope.kgr");
    let argument = runtime
        .resolve_type_arguments(layout.module(), slice::from_ref(&nominal))
        .unwrap()
        .pop()
        .unwrap();
    let group = runtime.gc.alloc_operation_group(vec![]).unwrap();
    let mut parent = EnvironmentRecord::new(
        runtime.definition_context(),
        vec![parameter.clone()],
        vec![argument],
    )
    .unwrap();
    parent.add_receiver(&runtime.gc, group).unwrap();
    let parent = runtime.gc.alloc_environment(parent).unwrap();
    let parent_id = parent.id;
    let mut child = EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap();
    child.include(Some(parent.clone())).unwrap();
    let child = runtime.gc.alloc_environment(child).unwrap();
    let child_id = child.id;
    let expression = Ty::Tuple(vec![parameter.as_type()]);
    let retained = runtime
        .type_arguments(layout.module(), Some(child.types.clone()), &[expression])
        .unwrap()
        .pop()
        .unwrap();
    assert!(retained.has_origin());
    // Warm derived facts before collecting their supplying execution environments.
    let cached_element = retained.parameter(&runtime, layout.module(), 0).unwrap();
    drop((child, parent));
    assert_eq!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_operation_groups,
        1
    );
    assert!(runtime.gc.environment(parent_id).is_none());
    assert!(runtime.gc.environment(child_id).is_none());
    // Transferring immutable facts must not revive their collected environments.
    let retained = thread::spawn(move || retained).join().unwrap();
    assert!(runtime.gc.environment(parent_id).is_none());
    assert!(runtime.gc.environment(child_id).is_none());
    let record = Value::Struct(runtime.alloc_struct(layout.clone(), vec![]).unwrap());
    assert_eq!(retained.ty(), &Ty::Tuple(vec![nominal.clone()]));
    assert!(retained.matches(
        &runtime,
        &Value::Tuple(vec![record.clone()]),
        layout.module()
    ));
    let element = retained.parameter(&runtime, layout.module(), 0).unwrap();
    assert_eq!(element.ty(), &nominal);
    assert!(element.matches(&runtime, &record, layout.module()));
    assert!(cached_element.matches(&runtime, &record, layout.module()));
}

#[test]
fn immutable_runtime_descriptors_are_send_and_sync() {
    fn shareable<T: Send + Sync>() {}
    shareable::<TypeArgument>();
    shareable::<TypeBindings>();
    shareable::<TypeEnvironment>();
    shareable::<ScopedSignature>();
    shareable::<OperationBindings>();
    shareable::<StructLayoutRef>();
    shareable::<EnumVariantRef>();
    shareable::<StorageType>();
    shareable::<StoredCallable>();
    shareable::<NativeCursor>();
    shareable::<Value>();
}
