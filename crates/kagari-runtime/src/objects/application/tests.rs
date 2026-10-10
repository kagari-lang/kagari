use crate::{
    RootedInterfaceMethod, Runtime,
    error::RuntimeError,
    execution_metadata::{
        MetadataRoot, application_key::ApplicationArguments, call_contracts::InterfaceCallSite,
    },
    frame::{
        transfer::ReturnValue,
        types::{EnvironmentRecord, operations::OperationBindings},
    },
    module::{LoadedModule, execution::calls::PreparedCallTarget},
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
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
use kagari_common::identity::table::DefinitionId;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::types::PublicItem;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::sync::Arc;

fn load_source(source: &str, module: Option<&NativeModule>) -> (Runtime, LoadedModule) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("group.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    if let Some(module) = module {
        analysis.set_native_modules(vec![Arc::new(module.to_declaration().unwrap())]);
    }
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let code = lower_program_to_bytecode(&mir).unwrap();
    let mut runtime = Runtime::default();
    if let Some(module) = module {
        module.install(&mut runtime).unwrap();
    }
    let loaded = runtime.load_program("group", code).unwrap();
    (runtime, loaded)
}

#[test]
fn cached_native_application_reuses_its_receiver_environment_and_reclaims_the_cycle() {
    for teardown in [false, true] {
        let mut module = ModuleBuilder::new("example::defaults", &DeclarationCatalog::default());
        let mut declaration = module.define_trait("Read");
        let read = declaration
            .define_method(MethodDecl::instance("read").returns(Type::i32()))
            .unwrap();
        let twice = declaration
            .define_method(MethodDecl::instance("twice").returns(Type::i32()))
            .unwrap();
        let required = declaration.operation(&read).unwrap();
        let selected = declaration
            .method(&twice, |method| Ok(method.requires(required)))
            .unwrap();
        declaration
            .bind_default_with(
                twice,
                NativeBinding::new(
                    vec![Codec::Value],
                    Codec::Scalar(Type::i32().abi().clone()),
                    move |cx| {
                        let receiver = cx.argument(0)?;
                        let target = cx.selected(&selected)?;
                        let Value::I32(value) = cx.call_values(target, &[receiver])? else {
                            return Err(RuntimeError::module_validation("checked Read result"));
                        };
                        Ok(Value::I32(value * 2))
                    },
                ),
            )
            .unwrap();
        declaration.finish().unwrap();
        let module = module.finish().unwrap();
        let (runtime, loaded) = load_source(
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
            .bind_table_operations(&snapshot.receiver_table)
            .unwrap();
        let member = snapshot
            .methods
            .iter()
            .flatten()
            .find(|method| loaded.definition_name(method.method) == Some("twice"))
            .unwrap()
            .method;
        let requirement = NativeCallableRequirement {
            receiver: snapshot.concrete_type.clone(),
            interface: snapshot.interface_type.clone(),
            member,
            arguments: vec![],
        };
        let operation_id = runtime
            .gc
            .operation_group(group)
            .unwrap()
            .operation(&requirement)
            .unwrap();
        let operation = runtime.gc.bound_operation(operation_id).unwrap();
        let prepare = || {
            let method = MethodInvocation::from_operation(&runtime, operation_id).unwrap();
            let method = runtime
                .apply_method_invocation(
                    method,
                    &ApplicationArguments::new(&loaded, vec![]).unwrap(),
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
        assert!(environment.operation(&runtime.gc, &requirement).is_some());
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
                .stage_reload_verified_program(&loaded, "group", loaded.verified_program().clone())
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
}

fn generic_fixture() -> (Runtime, LoadedModule, usize) {
    let (runtime, loaded) = load_source(
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
    (runtime, loaded, table)
}

fn apply_generic(
    runtime: &Runtime,
    loaded: &LoadedModule,
    value: Value,
    ty: Ty<DefinitionId>,
) -> RootedInterfaceMethod {
    let Value::Interface(id) = value else {
        panic!("interface fixture")
    };
    let interface = runtime
        .gc
        .interface_snapshot(id)
        .unwrap()
        .interface_type
        .clone();
    let arguments = runtime.resolve_type_arguments(loaded, &[ty]).unwrap();
    runtime
        .resolve_interface_method_slot(&value, &interface, 0, &arguments)
        .unwrap()
}

#[test]
fn applied_facts_reuse_receiver_independent_identity_and_distinguish_types_and_versions() {
    let (runtime, loaded, table) = generic_fixture();
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
        .stage_reload_verified_program(&loaded, "group", loaded.verified_program().clone())
        .unwrap();
    let latest = runtime.publish_staged_reload(candidate).unwrap();
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
fn polymorphic_retention_is_bounded_and_eviction_preserves_active_applications() {
    let (runtime, loaded, table) = generic_fixture();
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
    assert_eq!(runtime.gc.stats().method_applications, 129);
    drop(retained);
    runtime.collect_garbage().unwrap();
    // With the external lease gone and the cache entry evicted, only the active
    // window protects the applied signature needed to validate the return.
    assert!(runtime.gc.method_application(application).is_some());
    assert_eq!(runtime.gc.stats().method_applications, 129);
    assert_eq!(
        stack
            .finish_return(&runtime, ReturnValue::general(Value::I32(7)))
            .unwrap(),
        Some(Value::I32(7))
    );
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc.method_application(application).is_none());
    assert_eq!(runtime.gc.stats().method_applications, 128);
    let Value::Interface(id) = value else {
        unreachable!()
    };
    assert!(runtime.gc.interface_snapshot(id).is_none());
}

#[test]
fn optional_application_retention_expires_with_abandoned_witness_providers() {
    for publish in [false, true] {
        let (runtime, loaded, table) = generic_fixture();
        let value = runtime
            .make_interface(&loaded, table, Value::I32(1))
            .unwrap();
        let root = runtime.root_value(value).unwrap();
        let candidate = runtime
            .stage_reload_verified_program(&loaded, "group", loaded.verified_program().clone())
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
            .bind_table_operations(&snapshot.receiver_table)
            .unwrap();
        let interface = snapshot.interface_type.clone();
        drop(snapshot);
        let mut operations = OperationBindings::default();
        operations.receiver(&runtime.gc, group).unwrap();
        let arguments = runtime
            .resolve_type_arguments::<DefinitionId>(&loaded, &[Ty::Builtin(BuiltinType::I32)])
            .unwrap();
        let method = runtime
            .prepare_interface_method_slot(&value, &interface, 0, &arguments, operations)
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
}

#[test]
fn witness_preparation_reuses_checked_selections_and_retires_with_its_program() {
    let (runtime, mut loaded) = load_source(
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
                function
                    .instructions
                    .iter()
                    .enumerate()
                    .find_map(|(pc, instruction)| match instruction {
                        BytecodeInstruction::Call {
                            callee: CallTarget::InterfaceMethod { contract, .. },
                            ..
                        } if !contract.operations.is_empty() => Some(InterfaceCallSite {
                            function: function.id,
                            pc,
                        }),
                        _ => None,
                    })
            })
            .unwrap();
        let closed = || {
            let PreparedCallTarget::Interface { index, .. } =
                loaded.execution().functions[site.function.index()].calls[&site.pc].target
            else {
                unreachable!("interface call fixture");
            };
            runtime
                .modules
                .linked_function(&loaded, site.function, None)
                .unwrap()
                .call(index)
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
            last_scope = Some(scope);
        }
        runtime.collect_garbage().unwrap();
        let groups = runtime.gc.stats().operation_groups;
        assert_eq!(groups, 129);
        assert!(first.validate(&runtime.gc));
        // Retaining an index key cannot resurrect a stale caller environment.
        assert!(
            runtime
                .prepare_scoped_interface_call(&loaded, last_scope.unwrap(), site)
                .is_err()
        );
        let candidate = runtime
            .stage_reload_verified_program(&loaded, "group", loaded.verified_program().clone())
            .unwrap();
        let retired = loaded.clone();
        loaded = runtime.publish_staged_reload(candidate).unwrap();
        runtime.collect_garbage().unwrap();
        assert!(first.validate(&runtime.gc));
        drop(root);
        let collected = runtime.collect_garbage().unwrap();
        assert_eq!(collected.reclaimed_operation_groups, groups);
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
