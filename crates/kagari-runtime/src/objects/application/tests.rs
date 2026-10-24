use crate::{
    RootedInterfaceMethod, Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    execution_metadata::{
        MetadataRoot, application_key::ApplicationArguments, call_contracts::InterfaceCallSite,
    },
    frame::{
        driver::ReturnAction,
        transfer::ReturnValue,
        types::{
            EnvironmentRecord,
            compatibility::TypeView,
            construction::ConstructionScope,
            operations::{OperationBindings, OperationSegment},
        },
    },
    module::{ExecutableModule, Program, linked_execution::calls::LinkedCall},
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        declarations::MethodDecl,
        module::NativeModule,
        types::Type,
    },
    objects::invocation::MethodInvocation,
    value::Value,
};
use kagari_bytecode::module::CallableTarget;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::{
    callable::construction::evidence::ConstructionRequirement, types::PublicItem,
};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use rstest::rstest;
use std::sync::Arc;

fn load_source(
    source: &str,
    module: Option<&NativeModule>,
) -> (Runtime, Program, ExecutableModule) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("group.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    if let Some(module) = module {
        analysis.set_native_modules(vec![Arc::new(module.to_declaration())]);
    }
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let code = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let mut runtime = Runtime::default();
    if let Some(module) = module {
        module.install(&mut runtime).unwrap();
    }
    let loaded_program = runtime.load_program("group", code).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    (runtime, loaded_program, loaded)
}

#[rstest]
#[case::collect(false)]
#[case::teardown(true)]
fn cached_native_application_reuses_its_receiver_environment_and_reclaims_the_cycle(
    #[case] teardown: bool,
) {
    let mut module =
        ModuleBuilder::new("example::defaults", &DeclarationCatalog::default()).unwrap();
    let mut declaration = module.define_trait("Read").unwrap();
    let read = declaration
        .define_method(MethodDecl::instance("read").returns(Type::i32()))
        .unwrap();
    let twice = declaration
        .define_method(MethodDecl::instance("twice").returns(Type::i32()))
        .unwrap();
    let required = declaration.operation(&read).unwrap();
    let selected = declaration
        .method(&twice, |method| method.requires(required))
        .unwrap();
    declaration
        .bind(
            twice,
            NativeBinding::new(vec![Codec::Value], Codec::Exact(Type::i32()), move |cx| {
                let receiver = cx.argument(0)?;
                let target = cx.selected(&selected)?;
                let Value::I32(value) = cx.call_values(target, &[receiver])? else {
                    return Err(RuntimeError::module_validation("checked Read result"));
                };
                Ok(Value::I32(value * 2))
            }),
        )
        .unwrap();
    declaration.finish().unwrap();
    let module = module.finish().unwrap();
    let (runtime, loaded_program, loaded) = load_source(
        r#"
    use example::defaults::Read;
    impl Read for i32 { fn read(self) -> i32 { self } }
    fn main() -> Read { 7 }
    "#,
        Some(&module),
    );
    let table = loaded
        .bytecode
        .interface_tables
        .iter()
        .position(|table| {
            table
                .methods
                .iter()
                .any(|method| loaded.definition_name(method.method) == Some("twice"))
        })
        .unwrap();
    let Value::Interface(id) = runtime
        .make_interface(&loaded, table, Value::I32(7))
        .unwrap()
    else {
        panic!("interface fixture");
    };
    let snapshot = runtime.gc.interface_snapshot(id).unwrap();
    let group = runtime
        .bind_table_operations(&snapshot.description.receiver_table)
        .unwrap();
    let member = snapshot
        .description
        .methods
        .iter()
        .flatten()
        .find(|method| loaded.definition_name(method.method) == Some("twice"))
        .unwrap()
        .method;
    let requirement = NativeCallableRequirement {
        receiver: snapshot.description.concrete_type.clone(),
        interface: snapshot.description.interface_type().clone(),
        member,
        arguments: vec![],
    };
    let operation_id = runtime
        .gc
        .operation_group(group)
        .unwrap()
        .operation(
            &requirement,
            &TypeView::new(&requirement.receiver, &loaded, None)
                .identity()
                .unwrap(),
        )
        .unwrap();
    let operation = runtime.gc.bound_operation(operation_id).unwrap();
    let prepare = || {
        let method = MethodInvocation::from_operation(&runtime, operation_id).unwrap();
        let method = runtime
            .apply_method_invocation(
                method,
                &ApplicationArguments::new(&loaded, vec![], ConstructionScope::new(&loaded))
                    .unwrap(),
                Default::default(),
            )
            .unwrap();
        let mut edges = Vec::new();
        method.append_metadata(&mut edges);
        (runtime.root_metadata(edges).unwrap(), method)
    };
    let (first_root, first) = prepare();
    let (second_root, second) = prepare();
    let environment = first
        .view(&runtime)
        .unwrap()
        .environment()
        .cloned()
        .unwrap();
    assert_eq!(
        environment.id,
        second.view(&runtime).unwrap().environment().unwrap().id
    );
    assert_eq!(
        environment.id,
        runtime
            .gc
            .method_application(first.application.unwrap())
            .unwrap()
            .environment
            .as_ref()
            .unwrap()
            .id
    );
    assert!(
        environment
            .operation(&runtime.gc, &requirement, &operation.receiver_identity)
            .is_some()
    );
    let environment_id = environment.id;
    let runtime_owner = runtime.resources().lifetime_probe();
    let application_id = first.application.unwrap();
    assert_eq!(first.application, Some(application_id));
    assert_eq!(second.application, Some(application_id));
    drop((operation, snapshot, first_root));
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc.stats().operation_groups, 1);
    assert!(runtime.gc.environment(environment_id).is_some());
    assert!(runtime.gc.method_application(application_id).is_some());
    assert_eq!(runtime.gc.stats().method_applications, 1);
    if teardown {
        // A prepared identity can survive runtime teardown without owning the
        // operation or its cached application. This handle has no applied view.
        let retained = MethodInvocation::from_operation(&runtime, operation_id).unwrap();
        let mut edges = Vec::new();
        retained.append_metadata(&mut edges);
        let retained_root = runtime.root_metadata(edges).unwrap();
        drop(second_root);
        drop(runtime);
        assert!(runtime_owner.upgrade().is_none());
        let foreign = Runtime::default();
        assert!(retained.view(&foreign).is_err());
        drop(retained_root);
    } else {
        let candidate = runtime
            .stage_reload_verified_program(&loaded_program, None, loaded.verified_program().clone())
            .unwrap();
        runtime.publish_staged_reload(candidate).unwrap();
        // The retained method protects the old program/application cycle;
        // after its last root is dropped, the entire retired cycle is dead.
        runtime.collect_garbage().unwrap();
        assert!(runtime.gc.method_application(application_id).is_some());
        drop(second_root);
        assert_eq!(
            runtime
                .collect_garbage()
                .unwrap()
                .reclaimed_operation_groups,
            1
        );
        assert!(runtime.gc.environment(environment_id).is_none());
        assert!(runtime.gc.method_application(application_id).is_none());
        assert_eq!(runtime.gc.stats().method_applications, 0);
    }
}

fn generic_fixture() -> (Runtime, Program, ExecutableModule, usize) {
    let (runtime, loaded_program, loaded) = load_source(
        r#"
        struct Marker {}
        fn marker() -> Marker { Marker {} }
        trait Forward { fn forward<T>(self, value: T) -> T { value } }
        impl Forward for i32 {}
        impl Forward for i64 {}
        fn main() -> i32 { val receiver: Forward = 1; receiver.forward(7) }
        fn other() -> Forward { 2i64 }
        "#,
        None,
    );
    let table = loaded
        .bytecode
        .interface_tables
        .iter()
        .position(|table| loaded.bytecode.public_items.iter().any(|item| matches!(item, PublicItem::InterfaceTable(template) if template.declaration == table.declaration && template.for_type == Ty::Builtin(BuiltinType::I32))))
        .unwrap();
    (runtime, loaded_program, loaded, table)
}

fn apply_generic(
    runtime: &Runtime,
    loaded: &ExecutableModule,
    value: Value,
    ty: Ty,
) -> RootedInterfaceMethod {
    let Value::Interface(id) = value else {
        panic!("interface fixture")
    };
    let interface = runtime
        .gc
        .interface_snapshot(id)
        .unwrap()
        .description
        .interface_type()
        .clone();
    let arguments = runtime.resolve_type_arguments(loaded, &[ty]).unwrap();
    runtime
        .resolve_interface_method_slot(&value, &interface, 0, &arguments)
        .unwrap()
}

#[test]
fn applied_facts_reuse_receiver_independent_identity_and_distinguish_types_and_versions() {
    let (runtime, loaded_program, loaded, table) = generic_fixture();
    let first_value = runtime
        .make_interface(&loaded, table, Value::I32(1))
        .unwrap();
    let second_value = runtime
        .make_interface(&loaded, table, Value::I32(2))
        .unwrap();
    let first = apply_generic(
        &runtime,
        &loaded,
        first_value,
        Ty::Builtin(BuiltinType::I32),
    );
    let second = apply_generic(
        &runtime,
        &loaded,
        second_value,
        Ty::Builtin(BuiltinType::I32),
    );
    assert_eq!(first.invocation.application, second.invocation.application);
    let wide = apply_generic(
        &runtime,
        &loaded,
        second_value,
        Ty::Builtin(BuiltinType::I64),
    );
    assert_ne!(first.invocation.application, wide.invocation.application);
    assert_eq!(
        wide.return_type(&runtime).unwrap(),
        Ty::Builtin(BuiltinType::I64)
    );
    let other_table = loaded
        .bytecode
        .interface_tables
        .iter()
        .position(|table| loaded.bytecode.public_items.iter().any(|item| matches!(item, PublicItem::InterfaceTable(template) if template.declaration == table.declaration && template.for_type == Ty::Builtin(BuiltinType::I64))))
        .unwrap();
    let other_value = runtime
        .make_interface(&loaded, other_table, Value::I64(3))
        .unwrap();
    let other = apply_generic(
        &runtime,
        &loaded,
        other_value,
        Ty::Builtin(BuiltinType::I32),
    );
    assert_ne!(first.invocation.application, other.invocation.application);
    let candidate = runtime
        .stage_reload_verified_program(&loaded_program, None, loaded.verified_program().clone())
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();

    let latest = runtime.select_program(&loaded_program).unwrap();
    let fresh_value = runtime
        .make_interface(&latest, table, Value::I32(4))
        .unwrap();
    let fresh = apply_generic(
        &runtime,
        &latest,
        fresh_value,
        Ty::Builtin(BuiltinType::I32),
    );
    assert_ne!(first.invocation.application, fresh.invocation.application);
    let nominal = Ty::Struct(NominalTy {
        declaration: loaded.bytecode.structures[0].declaration,
        arguments: vec![],
        associated_types: Default::default(),
    });
    let old_type = apply_generic(
        &runtime,
        &loaded,
        first_value,
        Ty::Tuple(vec![nominal.clone()]),
    );
    let new_arguments = runtime
        .resolve_type_arguments(&latest, &[Ty::Tuple(vec![nominal])])
        .unwrap();
    let new_type = runtime
        .resolve_interface_method_slot(&first_value, first.interface_type(), 0, &new_arguments)
        .unwrap();
    // Identical printed types and declarations retain distinct supplying scopes.
    assert_eq!(
        old_type.return_type(&runtime).unwrap(),
        new_type.return_type(&runtime).unwrap()
    );
    assert_ne!(
        old_type.invocation.application,
        new_type.invocation.application
    );

    runtime.collect_garbage().unwrap();
    assert!(first.return_type(&runtime).is_ok());
    let application = first.invocation.application.unwrap();
    drop((first, second, wide, other, old_type, new_type));
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.method_application(application).is_none());
    assert!(fresh.return_type(&runtime).is_ok());
}

#[test]
fn retained_method_transports_binders_witnesses_and_constructions_after_impl_reordering() {
    let first_impl =
        "impl Forward for i32 { fn forward<T: Read>(self, value: T) -> Read { value } }";
    let second_impl =
        "impl Forward for i64 { fn forward<T: Read>(self, value: T) -> Read { value } }";
    let source = format!(
        r#"
        trait Read {{ fn read(self) -> i32; }}
        impl Read for i32 {{ fn read(self) -> i32 {{ self }} }}
        trait Forward {{ fn forward<T: Read>(self, value: T) -> Read; }}
        {first_impl}
        {second_impl}
        fn main() -> i32 {{ val receiver: Forward = 0; receiver.forward(7).read() }}
        fn other() -> Forward {{ 2i64 }}
    "#
    );
    let (mut runtime, program, old) = load_source(&source, None);
    let table = old
        .bytecode
        .interface_tables
        .iter()
        .position(|table| {
            old.bytecode.public_items.iter().any(|item| {
                matches!(item,
            PublicItem::InterfaceTable(template)
            if template.declaration == table.declaration
                && template.for_type == Ty::Builtin(BuiltinType::I32)
                && template.methods.iter().any(|method| method.name == "forward"))
            })
        })
        .unwrap();
    let value = runtime.make_interface(&old, table, Value::I32(0)).unwrap();
    let root = runtime.root_value(value).unwrap();
    let call = old
        .bytecode
        .functions
        .iter()
        .find_map(|function| {
            let links = runtime.modules.linked_function(&old, function.id, None)?;
            (0..old.execution().functions[function.id.index()].calls.len()).find_map(|ordinal| {
                let call = links.call(ordinal)?;
                (!call.operations.is_empty()).then(|| call.clone())
            })
        })
        .unwrap();
    let Value::Interface(id) = value else {
        unreachable!()
    };
    let interface = runtime
        .gc
        .interface_snapshot(id)
        .unwrap()
        .description
        .interface_type()
        .clone();
    let method = runtime
        .prepare_interface_method_slot(
            &value,
            &interface,
            0,
            call.arguments.values(),
            call.operations.clone(),
            call.arguments.constructions().clone(),
        )
        .unwrap();
    let semantic_application = method.invocation.application;
    let before = method.view(&runtime).unwrap();
    let old_body = before.selected.generic().unwrap().clone();
    let old_target = before.target();
    drop(before);
    let admitted = method.admit(&runtime).unwrap();
    let admitted_environment = admitted.view(&runtime).unwrap().environment().unwrap().id;
    let independent = runtime
        .load_verified_program("independent", old.verified_program().clone())
        .unwrap();
    let independent = runtime.select_program(&independent).unwrap();
    let foreign_selection = runtime
        .select_call_route(&independent.bind_call_route(old_target).unwrap())
        .unwrap();
    assert!(
        call.operations
            .transported(&runtime, &foreign_selection)
            .is_err()
    );
    assert!(
        call.arguments
            .constructions()
            .transported(&runtime, &foreign_selection)
            .is_err()
    );
    let interface_type = Ty::Trait(interface.clone());
    assert!(
        !TypeView::new(&interface_type, &old, None)
            .identity()
            .unwrap()
            .same_contract(
                &TypeView::new(&interface_type, &independent, None)
                    .identity()
                    .unwrap()
            )
    );
    let changed = source
        .replace(
            &format!("{first_impl}\n        {second_impl}"),
            &format!("{second_impl}\n        {first_impl}"),
        )
        .replace("{ self }", "{ self + 10 }");
    let (_, _, candidate) = load_source(&changed, None);
    let candidate = runtime
        .stage_reload_verified_program(&program, None, candidate.verified_program().clone())
        .unwrap();
    {
        let _session = runtime.begin_candidate_initialization(&candidate).unwrap();
        assert_eq!(
            admitted.view(&runtime).err().unwrap().kind(),
            RuntimeErrorKind::ExecutionPhaseViolation,
            "exact admission must not bypass candidate isolation"
        );
    }
    runtime.publish_staged_reload(candidate).unwrap();
    let current = runtime.select_program(&program).unwrap();
    runtime.collect_garbage().unwrap();
    {
        let exact = admitted.view(&runtime).unwrap();
        assert_eq!(
            exact.implementation().publication_token(),
            old.publication_token()
        );
        assert_eq!(exact.target(), old_target);
        assert_eq!(exact.environment().unwrap().id, admitted_environment);
        assert!(exact.environment().unwrap().types.matches(&old_body));
    }
    let view = method.view(&runtime).unwrap();
    assert_eq!(
        view.implementation().publication_token(),
        current.publication_token()
    );
    assert_eq!(
        view.proof_implementation().publication_token(),
        old.publication_token()
    );
    assert_eq!(method.invocation.application, semantic_application);
    let environment = view.environment().unwrap().clone();
    let body = view.selected.generic().unwrap();
    assert!(environment.types.matches(body));
    for (old_parameter, parameter) in old_body.parameters.iter().zip(&body.parameters) {
        assert_eq!(
            view.selected.mapping().map(old_parameter.owner).unwrap(),
            parameter.owner
        );
    }
    assert!(
        old_body.parameters != body.parameters || old_target != view.target(),
        "fixture must move executable placement or binder owners"
    );
    let operation_ids = call.operations.identity().unwrap();
    let OperationSegment::Selected(operation_id) = operation_ids[0] else {
        panic!("fixture must carry an explicit checked witness")
    };
    let operation = runtime.gc.bound_operation(operation_id).unwrap();
    let requirement = view
        .selected
        .mapping()
        .remap(&operation.requirement, &Default::default())
        .unwrap()
        .into_records();
    assert_eq!(
        environment.operation(&runtime.gc, &requirement, &operation.receiver_identity),
        Some(operation_id)
    );
    drop(operation);
    let selected_operation = MethodInvocation::from_operation(&runtime, operation_id).unwrap();
    assert_eq!(
        selected_operation
            .view(&runtime)
            .unwrap()
            .implementation()
            .publication_token(),
        current.publication_token()
    );
    let bound = &body.bounds[0];
    let required = ConstructionRequirement {
        receiver: bound.ty.clone(),
        interface: bound.constraints[0].clone(),
    };
    let selected_owner = view.implementation().clone();
    let selected_target = view.target();
    let environment_root = runtime
        .root_metadata(vec![MetadataRoot::Environment(environment.id)])
        .unwrap();
    drop(view);
    runtime.collect_garbage().unwrap();
    let result = runtime
        .make_bound_interface(&selected_owner, &environment, &required, Value::I32(7))
        .unwrap();
    let result_root = runtime.root_value(result).unwrap();
    let Value::Interface(result_id) = result else {
        unreachable!()
    };
    let result_interface = runtime
        .gc
        .interface_snapshot(result_id)
        .unwrap()
        .description
        .interface_type()
        .clone();
    let read = runtime
        .resolve_interface_method_slot(&result, &result_interface, 0, &[])
        .unwrap();
    assert_eq!(
        read.implementation(&runtime).unwrap().publication_token(),
        current.publication_token()
    );
    // The exact retained construction proof still belongs to the old publication.
    assert_eq!(
        read.view(&runtime)
            .unwrap()
            .proof_implementation()
            .publication_token(),
        old.publication_token()
    );
    let stack = runtime.enter_execution_stack(&current).unwrap();
    stack
        .push_interface_method(
            &runtime,
            method.clone(),
            &[Value::I32(0), Value::I32(7)],
            None,
        )
        .unwrap();
    assert_eq!(
        stack.current().unwrap().loaded().publication_token(),
        current.publication_token()
    );
    assert_eq!(stack.current().unwrap().target(), selected_target);
    assert!(matches!(selected_target, CallableTarget::Script(_)));
    drop((
        stack,
        environment_root,
        result_root,
        read,
        admitted,
        method,
        root,
    ));
    runtime.collect_garbage().unwrap();
    assert!(!runtime.is_quarantined());
}

#[test]
fn polymorphic_retention_is_bounded_and_eviction_preserves_active_applications() {
    let (runtime, _loaded_program, loaded, table) = generic_fixture();
    let value = runtime
        .make_interface(&loaded, table, Value::I32(1))
        .unwrap();
    let retained = apply_generic(&runtime, &loaded, value, Ty::Builtin(BuiltinType::I32));
    let application = retained.invocation.application.unwrap();
    let stack = runtime.enter_execution_stack(&loaded).unwrap();
    stack
        .push_interface_method(
            &runtime,
            retained.clone(),
            &[Value::I32(1), Value::I32(7)],
            None,
        )
        .unwrap();
    // More distinct applications than the retention limit. The older call stays
    // rooted through its host handle even when the program evicts its index edge.
    for length in 0..160 {
        drop(apply_generic(
            &runtime,
            &loaded,
            value,
            Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); length]),
        ));
    }
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.method_application(application).is_some());
    // The active frame also retains its separately admitted computation.
    assert_eq!(runtime.gc.stats().method_applications, 130);
    drop(retained);
    runtime.collect_garbage().unwrap();
    // With the external lease gone and the cache entry evicted, only the active
    // window protects the applied signature and exact computation needed on return.
    assert!(runtime.gc.method_application(application).is_some());
    assert_eq!(runtime.gc.stats().method_applications, 130);
    assert_eq!(
        stack
            .finish_return(&runtime, ReturnValue::general(Value::I32(7)))
            .unwrap(),
        ReturnAction::Complete(Value::I32(7))
    );
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.method_application(application).is_none());
    assert_eq!(runtime.gc.stats().method_applications, 128);
    let Value::Interface(id) = value else {
        unreachable!()
    };
    assert!(runtime.gc.interface_snapshot(id).is_none());
}

#[rstest]
#[case::abandoned(false)]
#[case::published(true)]
fn optional_application_retention_expires_with_abandoned_witness_providers(#[case] publish: bool) {
    let (runtime, loaded_program, loaded, table) = generic_fixture();
    let value = runtime
        .make_interface(&loaded, table, Value::I32(1))
        .unwrap();
    let root = runtime.root_value(value).unwrap();
    let candidate = runtime
        .stage_reload_verified_program(&loaded_program, None, loaded.verified_program().clone())
        .unwrap();
    let supplier = candidate.module().clone();
    let Value::Interface(id) = runtime
        .make_interface(&supplier, table, Value::I32(2))
        .unwrap()
    else {
        unreachable!()
    };
    let snapshot = runtime.gc.interface_snapshot(id).unwrap();
    let group = runtime
        .bind_table_operations(&snapshot.description.receiver_table)
        .unwrap();
    let interface = snapshot.description.interface_type().clone();
    drop(snapshot);
    let mut operations = OperationBindings::default();
    operations.receiver(&runtime.gc, group).unwrap();
    let arguments = runtime
        .resolve_type_arguments(&loaded, &[Ty::Builtin(BuiltinType::I32)])
        .unwrap();
    let method = runtime
        .prepare_interface_method_slot(
            &value,
            &interface,
            0,
            &arguments,
            operations,
            Default::default(),
        )
        .unwrap();
    let application = method.invocation.application.unwrap();
    let environment = runtime
        .gc
        .method_application(application)
        .unwrap()
        .environment
        .clone()
        .unwrap();
    let pending = runtime.gc.environment(environment.id).unwrap().clone();
    runtime.validate_environment(environment.id).unwrap();
    drop(method);
    if publish {
        runtime.publish_staged_reload(candidate).unwrap();
    } else {
        drop(candidate);
    }
    // A still-live environment ID must not admit an abandoned transitive
    // witness provider. Re-publication must reject it before installing an ID.
    assert_eq!(
        runtime.validate_environment(environment.id).is_ok(),
        publish
    );
    let before = runtime.gc.stats();
    assert_eq!(runtime.alloc_environment(pending).is_ok(), publish);
    if !publish {
        assert_eq!(runtime.gc.stats(), before);
    }
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.gc.method_application(application).is_some(),
        publish
    );
    assert!(!runtime.is_quarantined());
    drop(root);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.method_application(application).is_none());
}

#[test]
fn witness_preparation_reuses_checked_selections_and_retires_with_its_program() {
    let (runtime, loaded_program, mut loaded) = load_source(
        r#"
        trait Read { fn read(self) -> i32; }
        impl Read for i32 { fn read(self) -> i32 { self } }
        trait Forward { fn forward<T: Read>(self, value: T) -> i32 { value.read() } }
        impl Forward for i32 {}
        fn main() -> i32 { val receiver: Forward = 0; receiver.forward(7) }
        "#,
        None,
    );
    for _ in 0..3 {
        let site = loaded
            .bytecode
            .functions
            .iter()
            .find_map(|function| {
                let links = runtime
                    .modules
                    .linked_function(&loaded, function.id, None)?;
                let count = loaded.execution().functions[function.id.index()]
                    .calls
                    .len();
                (0..count).find_map(|ordinal| match links.call_contract(ordinal) {
                    Some(LinkedCall::Interface { contract, .. })
                        if !contract.operations.is_empty() =>
                    {
                        Some(InterfaceCallSite {
                            function: function.id,
                            ordinal,
                        })
                    }
                    _ => None,
                })
            })
            .unwrap();
        let closed = || {
            runtime
                .modules
                .linked_function(&loaded, site.function, None)
                .unwrap()
                .call(site.ordinal)
                .unwrap()
                .clone()
        };
        let first_call = closed();
        let second_call = closed();
        assert!(Arc::ptr_eq(&first_call, &second_call));
        let first = first_call.operations.clone();
        assert!(!first.is_empty());
        runtime.collect_garbage().unwrap();
        assert!(first.validate(&runtime.gc));
        // A caller-owned environment roots the selected old provider independently
        // of the optional program index. Retiring the index must not break it.
        let mut record =
            EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap();
        record.extend_operations(first.clone());
        let environment = runtime.alloc_environment(record).unwrap();
        let root = runtime
            .root_metadata(vec![MetadataRoot::Environment(environment.id)])
            .unwrap();
        // The bound applies across lexical scopes, not separately to each scope.
        // The explicitly rooted first selection survives eviction from the index.
        let mut evicted_scope = None;
        let mut last_scope = None;
        for _ in 0..160 {
            let scope = runtime
                .alloc_environment(
                    EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap(),
                )
                .unwrap();
            let call = runtime
                .prepare_scoped_interface_call(&loaded, scope.clone(), site)
                .unwrap();
            assert_ne!(first.identity(), call.operations.identity());
            evicted_scope.get_or_insert_with(|| scope.clone());
            last_scope = Some(scope);
        }
        runtime.collect_garbage().unwrap();
        let groups = runtime.gc.stats().operation_groups;
        assert_eq!(groups, 129);
        assert!(first.validate(&runtime.gc));
        // Retained construction capabilities trace their supplying environment.
        // An evicted call has no such edge; its stale scope cannot be resurrected.
        let last_scope = last_scope.unwrap();
        let evicted_scope = evicted_scope.unwrap();
        assert!(runtime.gc.environment(last_scope.id).is_some());
        assert!(runtime.gc.environment(evicted_scope.id).is_none());
        assert!(
            runtime
                .prepare_scoped_interface_call(&loaded, last_scope.clone(), site)
                .is_ok()
        );
        assert!(
            runtime
                .prepare_scoped_interface_call(&loaded, evicted_scope, site)
                .is_err()
        );
        let candidate = runtime
            .stage_reload_verified_program(&loaded_program, None, loaded.verified_program().clone())
            .unwrap();
        let retired = loaded.clone();
        runtime.publish_staged_reload(candidate).unwrap();
        loaded = runtime.select_program(&loaded_program).unwrap();
        runtime.collect_garbage().unwrap();
        assert!(first.validate(&runtime.gc));
        drop(root);
        let collected = runtime.collect_garbage().unwrap();
        assert_eq!(collected.reclaimed_operation_groups, groups);
        assert!(runtime.gc.environment(last_scope.id).is_none());
        // Linking prepares the new program's closed witness before publication.
        // All old groups are gone; only that distinct live program's group remains.
        assert_eq!(runtime.gc.stats().operation_groups, 1);
        assert!(
            runtime
                .modules
                .linked_function(&retired, site.function, None)
                .is_none()
        );
        let linked = runtime
            .modules
            .linked_function(&loaded, site.function, None)
            .unwrap();
        assert!(linked.call(0).unwrap().operations.validate(&runtime.gc));
        assert_ne!(
            first.identity(),
            linked.call(0).unwrap().operations.identity()
        );
        assert_eq!(runtime.gc.stats().environments, 0);
        assert!(!first.validate(&runtime.gc));
        assert!(!runtime.is_quarantined());
    }
}
