use super::*;
use crate::{
    execution::roots::ExecutionRoots,
    execution_metadata::managed::sources::MetadataSource,
    frame::types::{EnvironmentRecord, TypeEnvironment, operations::OperationBindings},
    module::{EnumVariantRef, StructLayoutRef},
    native::{callable::StoredCallable, storage_type::StorageType},
    test_support::layouts as layout_fixtures,
    value::{aggregates::tuples::TupleObject, carriers::ScopedValue},
    value_check,
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

fn binder(definitions: &DefinitionContext, module: &str) -> GenericParam {
    GenericParam {
        owner: definitions.intern(&owner(module)).unwrap(),
        position: 0,
    }
}

fn argument(kind: BuiltinType) -> TypeArgument {
    TypeArgument {
        data: Arc::new(TypeArgumentData {
            registry: None,
            ty: Ty::Builtin(kind),
            definitions: DefinitionContext::new().unwrap().snapshot(),
            origin: None,
            parameters: OnceLock::new(),
            variants: OnceLock::new(),
            identity: OnceLock::new(),
            admission: OnceLock::new(),
        }),
    }
}

#[test]
fn scoped_frame_binders_preserve_parent_scope_and_survive_context_owner_drop() {
    let definitions = DefinitionContext::new().unwrap();
    let parent = Arc::new(
        {
            let parameters = vec![binder(&definitions, "parent.kgr")];
            TypeBindings::new(
                &definitions.snapshot(),
                parameters,
                vec![argument(BuiltinType::I32)],
            )
        }
        .unwrap(),
    );
    let mut child = {
        let parameters = vec![binder(&definitions, "child.kgr")];
        TypeBindings::new(
            &definitions.snapshot(),
            parameters,
            vec![argument(BuiltinType::String)],
        )
    }
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
        {
            let parameters = vec![binder(&definitions, "owner.kgr")];
            TypeBindings::new(
                &definitions.snapshot(),
                parameters,
                vec![argument(BuiltinType::I32)],
            )
        }
        .unwrap(),
    );
    let mut duplicate = {
        let parameters = vec![binder(&definitions, "owner.kgr")];
        TypeBindings::new(
            &definitions.snapshot(),
            parameters,
            vec![argument(BuiltinType::String)],
        )
    }
    .unwrap();
    assert!(duplicate.include(Some(parent.clone())).is_err());
    let foreign = DefinitionContext::new().unwrap();
    let mut child = {
        let parameters = vec![binder(&foreign, "other.kgr")];
        TypeBindings::new(
            &foreign.snapshot(),
            parameters,
            vec![argument(BuiltinType::String)],
        )
    }
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
    runtime.collect_garbage().unwrap();
    let before = runtime.memory.stats().objects;
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let group = runtime
        .alloc_operation_group(execution, vec![], &[])
        .unwrap();
    let mut parent = EnvironmentRecord::new(
        &runtime.definition_context().snapshot(),
        vec![parameter.clone()],
        vec![argument],
    )
    .unwrap();
    parent.add_receiver(group.record()).unwrap();
    let parent = runtime
        .alloc_environment(
            execution,
            parent,
            &[MetadataSource::AdmittedOperations(&group)],
        )
        .unwrap();
    let parent_source = parent.source();
    let mut child =
        EnvironmentRecord::new(&runtime.definition_context().snapshot(), vec![], vec![]).unwrap();
    child.include(Some(parent.facts().clone())).unwrap();
    let child = runtime
        .alloc_environment(
            execution,
            child,
            &[MetadataSource::AdmittedEnvironment(&parent)],
        )
        .unwrap();
    let child_source = child.source();
    let expression = Ty::Tuple(vec![parameter.as_type()]);
    let retained = runtime
        .type_arguments(
            layout.module(),
            Some(child.facts().types.clone()),
            &[expression],
        )
        .unwrap()
        .pop()
        .unwrap();
    assert!(retained.has_origin());
    let cached_element = retained.parameter(&runtime, layout.module(), 0).unwrap();
    assert_eq!(runtime.memory.stats().objects, before + 3);
    drop((child, parent, group));
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.memory.stats().objects, before);
    let retained = thread::spawn(move || retained).join().unwrap();
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                assert!(slots.dependency(parent_source).is_err());
                assert!(slots.dependency(child_source).is_err());
                Ok(())
            })
        })
        .unwrap();
    let record = runtime.alloc_struct(layout.clone(), &[]).unwrap();
    let tuple = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let record = record.get(cx)?;
            let tuple = TupleObject::alloc(cx, &[record])?;
            ScopedValue::managed(tuple).retain(cx)
        })
        .unwrap();
    assert_eq!(retained.ty(), &Ty::Tuple(vec![nominal.clone()]));
    assert!(value_check::matches_rooted(&mut runtime, layout.module(), &retained, &tuple).unwrap());
    let element = retained.parameter(&runtime, layout.module(), 0).unwrap();
    assert_eq!(element.ty(), &nominal);
    assert!(value_check::matches_rooted(&mut runtime, layout.module(), &element, &record).unwrap());
    assert!(
        value_check::matches_rooted(&mut runtime, layout.module(), &cached_element, &record)
            .unwrap()
    );
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
}
