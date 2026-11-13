#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    RootedInterfaceMethod, Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    execution_metadata::{
        application_key::{ApplicationArguments, ApplicationKey},
        applications::{ApplicationId, ApplicationRecord},
        call_contracts::InterfaceCallSite,
        managed::sources::MetadataSource,
    },
    executor::Executor,
    frame::{
        driver::ReturnAction,
        transfer::ReturnValue,
        types::{
            EnvironmentRecord, TypeEnvironment,
            compatibility::TypeView,
            construction::ConstructionScope,
            operations::{OperationBindings, OperationSegment},
        },
    },
    module::{
        ExecutableModule, InstalledProgram, linked_execution::calls::LinkedCall,
        managed::source::RevisionSource,
    },
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{arguments::ArgumentValues, context::ConversionContext},
        declarations::MethodDecl,
        module::NativeModule,
        types::Type,
    },
    objects::{application::MethodApplicationInput, invocation::MethodInvocation},
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::module::CallableTarget;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::{
    callable::construction::evidence::ConstructionRequirement, types::PublicItem,
};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use rstest::rstest;
use std::sync::Arc;

fn load_source(
    source: &str,
    module: Option<&NativeModule>,
) -> (Runtime, InstalledProgram, ExecutableModule) {
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
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let code = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let mut runtime = Runtime::default();
    if let Some(module) = module {
        module.install(&mut runtime).unwrap();
    }
    let loaded_program = load_bytecode(&mut runtime, "group", code).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    (runtime, loaded_program, loaded)
}

fn application_id(method: &RootedInterfaceMethod) -> ApplicationId {
    method.invocation.application.as_ref().unwrap().id()
}

fn semantic_environment(runtime: &mut Runtime, method: &RootedInterfaceMethod) -> TypeEnvironment {
    let record = runtime
        .application_record_in(&method.sources().unwrap(), application_id(method))
        .unwrap();
    let ApplicationRecord::Semantic(application) = record.as_ref() else {
        panic!("semantic method application");
    };
    application.environment.as_ref().unwrap().clone()
}

#[rstest]
#[case::collect(false)]
#[case::teardown(true)]
fn protected_native_applications_reuse_environment_and_release_their_old_graphs(
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
                let Some(value) = cx.call_values(target, &[receiver])?.as_i32() else {
                    return Err(RuntimeError::module_validation("checked Read result"));
                };
                Ok(ValueView::from(value * 2))
            }),
        )
        .unwrap();
    declaration.finish().unwrap();
    let module = module.finish().unwrap();
    let (mut runtime, program, loaded) = load_source(
        include_str!("../../../tests/fixtures/metadata/native-application.kgr"),
        Some(&module),
    );
    runtime.collect_garbage().unwrap();
    let roots = runtime.gc_stats().root_registrations;
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
    let value = runtime
        .make_interface(&loaded, table, &RootedValue::from(7i32))
        .unwrap();
    let description = runtime.interface_description(&value).unwrap();
    let slot = description
        .methods
        .iter()
        .position(|method| {
            method
                .as_ref()
                .is_some_and(|method| loaded.definition_name(method.method) == Some("twice"))
        })
        .unwrap();
    let first = runtime
        .resolve_interface_method_slot(&value, description.interface_type(), slot, &[])
        .unwrap();
    let second = {
        let sources = first.sources().unwrap();
        runtime
            .prepare_interface_method_slot(
                &value,
                description.interface_type(),
                slot,
                MethodApplicationInput {
                    arguments: &[],
                    operations: Default::default(),
                    constructions: Default::default(),
                    sources: &sources,
                },
            )
            .unwrap()
    };
    assert_eq!(application_id(&first), application_id(&second));
    let environment = semantic_environment(&mut runtime, &first);
    assert_eq!(
        environment.id,
        semantic_environment(&mut runtime, &second).id
    );
    let snapshot = runtime
        .environment_snapshot_in(&second.sources().unwrap(), &environment)
        .unwrap();
    assert_eq!(snapshot.groups.len(), 1);
    let (operation_id, operation) = snapshot
        .groups
        .values()
        .flat_map(|group| {
            group
                .operation_ids()
                .map(move |id| (id, group.get(id).unwrap()))
        })
        .find(|(_, operation)| loaded.definition_name(operation.requirement.member) == Some("read"))
        .unwrap();
    assert_eq!(
        snapshot.operation(&operation.requirement_key, &operation.receiver_identity),
        Some(operation_id)
    );
    let record = first.invocation.application.as_ref().unwrap();
    let expired = Arc::downgrade(record.record());
    let runtime_owner = runtime.resources().lifetime_probe();
    drop((snapshot, description, first, value));
    runtime.collect_garbage().unwrap();
    assert_eq!(
        semantic_environment(&mut runtime, &second).id,
        environment.id
    );
    assert!(expired.upgrade().is_some());
    if teardown {
        drop(runtime);
        assert!(runtime_owner.upgrade().is_none());
        let mut foreign = Runtime::default();
        assert_eq!(
            second.return_type(&mut foreign).unwrap_err().kind(),
            RuntimeErrorKind::ModuleValidation
        );
        assert!(!foreign.is_quarantined());
        drop(second);
    } else {
        let candidate = runtime
            .stage_reload_verified_program(&program, None, loaded.verified_program().clone())
            .unwrap();
        runtime.publish_staged_reload(candidate).unwrap();
        let current = runtime.select_program(&program).unwrap();
        runtime.collect_garbage().unwrap();
        assert!(runtime.modules().loaded(loaded.key()).is_some());
        assert_eq!(
            semantic_environment(&mut runtime, &second).id,
            environment.id
        );
        drop(second);
        runtime.collect_garbage().unwrap();
        assert!(expired.upgrade().is_none());
        assert!(runtime.modules().loaded(loaded.key()).is_none());
        assert!(runtime.modules().loaded(current.key()).is_some());
        assert_eq!(runtime.gc_stats().root_registrations, roots);
    }
}

fn generic_fixture() -> (Runtime, InstalledProgram, ExecutableModule, usize) {
    let (runtime, loaded_program, loaded) = load_source(
        include_str!("../../../tests/fixtures/metadata/application-generics.kgr"),
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
    runtime: &mut Runtime,
    loaded: &ExecutableModule,
    value: &RootedValue,
    ty: Ty,
) -> RootedInterfaceMethod {
    let description = runtime.interface_description(value).unwrap();
    let arguments = runtime.resolve_type_arguments(loaded, &[ty]).unwrap();
    runtime
        .resolve_interface_method_slot(value, description.interface_type(), 0, &arguments)
        .unwrap()
}

#[test]
fn protected_applications_reuse_facts_across_receivers_and_distinguish_types_and_versions() {
    let (mut runtime, loaded_program, loaded, table) = generic_fixture();
    let first_value = runtime
        .make_interface(&loaded, table, &RootedValue::from(1i32))
        .unwrap();
    let second_value = runtime
        .make_interface(&loaded, table, &RootedValue::from(2i32))
        .unwrap();
    let first = apply_generic(
        &mut runtime,
        &loaded,
        &first_value,
        Ty::Builtin(BuiltinType::I32),
    );
    let second = {
        let arguments = runtime
            .resolve_type_arguments(&loaded, &[Ty::Builtin(BuiltinType::I32)])
            .unwrap();
        runtime
            .prepare_interface_method_slot(
                &second_value,
                first.interface_type(),
                0,
                MethodApplicationInput {
                    arguments: &arguments,
                    operations: Default::default(),
                    constructions: Default::default(),
                    sources: &first.sources().unwrap(),
                },
            )
            .unwrap()
    };
    assert_eq!(application_id(&first), application_id(&second));
    let wide = apply_generic(
        &mut runtime,
        &loaded,
        &second_value,
        Ty::Builtin(BuiltinType::I64),
    );
    assert_ne!(application_id(&first), application_id(&wide));
    assert_eq!(
        wide.return_type(&mut runtime).unwrap(),
        Ty::Builtin(BuiltinType::I64)
    );
    let other_table = loaded
        .bytecode
        .interface_tables
        .iter()
        .position(|table| loaded.bytecode.public_items.iter().any(|item| matches!(item, PublicItem::InterfaceTable(template) if template.declaration == table.declaration && template.for_type == Ty::Builtin(BuiltinType::I64))))
        .unwrap();
    let other_value = runtime
        .make_interface(&loaded, other_table, &RootedValue::from(3i64))
        .unwrap();
    let other = apply_generic(
        &mut runtime,
        &loaded,
        &other_value,
        Ty::Builtin(BuiltinType::I32),
    );
    assert_ne!(application_id(&first), application_id(&other));
    let candidate = runtime
        .stage_reload_verified_program(&loaded_program, None, loaded.verified_program().clone())
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();

    let latest = runtime.select_program(&loaded_program).unwrap();
    let fresh_value = runtime
        .make_interface(&latest, table, &RootedValue::from(4i32))
        .unwrap();
    let fresh = apply_generic(
        &mut runtime,
        &latest,
        &fresh_value,
        Ty::Builtin(BuiltinType::I32),
    );
    assert_ne!(application_id(&first), application_id(&fresh));
    let nominal = Ty::Struct(NominalTy {
        declaration: loaded.bytecode.structures[0].declaration,
        arguments: vec![],
        associated_types: Default::default(),
    });
    let old_type = apply_generic(
        &mut runtime,
        &loaded,
        &first_value,
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
        old_type.return_type(&mut runtime).unwrap(),
        new_type.return_type(&mut runtime).unwrap()
    );
    assert_ne!(application_id(&old_type), application_id(&new_type));

    runtime.collect_garbage().unwrap();
    assert!(first.return_type(&mut runtime).is_ok());
    let expired = Arc::downgrade(first.invocation.application.as_ref().unwrap().record());
    drop((first, second, wide, other, old_type, new_type));
    runtime.collect_garbage().unwrap();
    assert!(expired.upgrade().is_none());
    assert!(fresh.return_type(&mut runtime).is_ok());
}

#[test]
fn retained_method_transports_binders_witnesses_and_constructions_after_impl_reordering() {
    let (mut runtime, program, old) = load_source(
        include_str!("../../../tests/fixtures/metadata/application-transport.kgr"),
        None,
    );
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
    let value = runtime
        .make_interface(&old, table, &RootedValue::from(0i32))
        .unwrap();
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
    let interface = runtime
        .interface_description(&value)
        .unwrap()
        .interface_type()
        .clone();
    let method = runtime
        .prepare_interface_method_slot(
            &value,
            &interface,
            0,
            MethodApplicationInput {
                arguments: call.arguments.values(),
                operations: call.operations.clone(),
                constructions: call.arguments.constructions().clone(),
                sources: &[MetadataSource::Revision {
                    owner: &old,
                    source: RevisionSource::Current,
                }],
            },
        )
        .unwrap();
    let semantic_application = application_id(&method);
    let before = method.view(&mut runtime).unwrap();
    let old_body = before.selected.generic().unwrap().clone();
    let old_target = before.target();
    assert!(
        before.environment().is_none(),
        "live selection does not prepare admission"
    );
    drop(before);
    let admitted = method.admit(&mut runtime).unwrap();
    let old_environment = admitted
        .view(&mut runtime)
        .unwrap()
        .environment()
        .unwrap()
        .clone();
    let admitted_environment = old_environment.id;
    let old_snapshot = runtime
        .environment_snapshot_in(&admitted.sources().unwrap(), &old_environment)
        .unwrap();
    let independent = runtime
        .load_verified_program("independent", old.verified_program().clone())
        .unwrap();
    let independent = runtime.select_program(&independent).unwrap();
    let foreign_selection = runtime
        .select_call_route(&independent.bind_call_route(old_target).unwrap())
        .unwrap();
    assert!(
        call.operations
            .transported(&runtime, &foreign_selection, &old_snapshot.groups)
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
    let (_, _, candidate) = load_source(
        include_str!("../../../tests/fixtures/metadata/application-transport-reloaded.kgr"),
        None,
    );
    let candidate = runtime
        .stage_reload_verified_program(&program, None, candidate.verified_program().clone())
        .unwrap();
    {
        let mut _session = runtime.begin_candidate_initialization(&candidate).unwrap();
        assert_eq!(
            admitted.view(_session.runtime_mut()).err().unwrap().kind(),
            RuntimeErrorKind::ExecutionPhaseViolation,
            "exact admission must not bypass candidate isolation"
        );
    }
    runtime.publish_staged_reload(candidate).unwrap();
    let current = runtime.select_program(&program).unwrap();
    runtime.collect_garbage().unwrap();
    {
        let exact = admitted.view(&mut runtime).unwrap();
        assert_eq!(
            exact.implementation().publication_token(),
            old.publication_token()
        );
        assert_eq!(exact.target(), old_target);
        assert_eq!(exact.environment().unwrap().id, admitted_environment);
        assert!(exact.environment().unwrap().types.matches(&old_body));
    }
    let current_admitted = method.admit(&mut runtime).unwrap();
    let view = current_admitted.view(&mut runtime).unwrap();
    assert_eq!(
        view.implementation().publication_token(),
        current.publication_token()
    );
    assert_eq!(
        view.proof_implementation().publication_token(),
        old.publication_token()
    );
    assert_eq!(application_id(&method), semantic_application);
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
    let operation = old_snapshot.groups[&operation_id.group]
        .get(operation_id)
        .unwrap();
    let requirement = view
        .selected
        .mapping()
        .remap_closed(&operation.requirement_key, &Default::default())
        .unwrap();
    let snapshot = runtime
        .environment_snapshot_in(&current_admitted.sources().unwrap(), &environment)
        .unwrap();
    assert_eq!(
        snapshot.operation(&requirement, &operation.receiver_identity),
        Some(operation_id)
    );
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let sources = current_admitted.sources().unwrap();
    let selected_operation =
        MethodInvocation::from_operation(&mut runtime, execution, operation_id, &sources).unwrap();
    assert_eq!(
        selected_operation
            .view(&mut runtime, &sources)
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
    drop((view, sources));
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let result = {
        let root = current_admitted.computation_root.as_ref().unwrap();
        let mut conversion = ConversionContext::for_selected(
            &mut runtime,
            &selected_owner,
            execution,
            RevisionSource::HostApplication(root),
            Some(MetadataSource::HostApplication(root)),
        )
        .unwrap();
        let value = conversion
            .make_bound_interface(&environment, &required, &ValueView::from(7i32))
            .unwrap();
        conversion.retain(&value).unwrap()
    };
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    let result_interface = runtime
        .interface_description(&result)
        .unwrap()
        .interface_type()
        .clone();
    let read = runtime
        .resolve_interface_method_slot(&result, &result_interface, 0, &[])
        .unwrap();
    assert_eq!(
        read.implementation(&mut runtime)
            .unwrap()
            .publication_token(),
        current.publication_token()
    );
    // The exact retained construction proof still belongs to the old publication.
    assert_eq!(
        read.view(&mut runtime)
            .unwrap()
            .proof_implementation()
            .publication_token(),
        old.publication_token()
    );
    let mut stack_activation = runtime.enter_execution_stack(&current).unwrap();
    stack_activation
        .with(|runtime, stack| {
            stack
                .push_interface_method(
                    runtime,
                    &method,
                    ArgumentValues::scalars(&[ValueView::from(7i32)]),
                    None,
                )
                .unwrap();
            assert_eq!(
                stack.current(runtime).unwrap().loaded().publication_token(),
                current.publication_token()
            );
            assert_eq!(stack.current(runtime).unwrap().target(), selected_target);
            assert!(matches!(selected_target, CallableTarget::Script(_)));
            drop((
                stack,
                result,
                current_admitted,
                old_snapshot,
                snapshot,
                read,
                admitted,
                method,
                value,
            ));
            runtime.collect_garbage().unwrap();
            assert!(!runtime.is_quarantined());

            Ok(())
        })
        .unwrap();
    let result = Executor::from_activation(stack_activation)
        .run(|runtime, stack, value| stack.retain_result(runtime, value))
        .unwrap();
    let description = runtime.interface_description(&result).unwrap();
    let method = description.methods[0].as_ref().unwrap().method;
    assert_eq!(
        runtime
            .invoke_interface_method(&result, &method, &[])
            .unwrap()
            .as_i32(),
        Some(17)
    );
}

#[test]
fn bounded_application_indexes_do_not_retain_nodes_and_active_frames_keep_their_computation() {
    let (mut runtime, _program, loaded, table) = generic_fixture();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let value = runtime
        .make_interface(&loaded, table, &RootedValue::from(1i32))
        .unwrap();
    let retained = apply_generic(&mut runtime, &loaded, &value, Ty::Builtin(BuiltinType::I32));
    let application = application_id(&retained);
    let expired = Arc::downgrade(retained.invocation.application.as_ref().unwrap().record());
    let arguments = runtime
        .resolve_type_arguments(&loaded, &[Ty::Builtin(BuiltinType::I32)])
        .unwrap();
    let key = ApplicationKey::new(
        retained.view(&mut runtime).unwrap().identity(),
        &ApplicationArguments::new(&loaded, arguments, ConstructionScope::new(&loaded)).unwrap(),
        None,
        &Default::default(),
    );
    assert_eq!(
        runtime.modules.method_application(&loaded, &key),
        Some(application)
    );
    let mut activation = runtime.enter_execution_stack(&loaded).unwrap();
    activation
        .with(|runtime, stack| {
            stack
                .push_interface_method(
                    runtime,
                    &retained,
                    ArgumentValues::scalars(&[ValueView::from(7i32)]),
                    None,
                )
                .unwrap();
            let computation = stack
                .current(runtime)
                .unwrap()
                .invocation()
                .unwrap()
                .computation()
                .unwrap()
                .clone();
            let mut unrooted = Vec::new();
            for length in 0..160 {
                let method = apply_generic(
                    runtime,
                    &loaded,
                    &value,
                    Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); length]),
                );
                unrooted.push(Arc::downgrade(
                    method.invocation.application.as_ref().unwrap().record(),
                ));
            }
            assert!(
                runtime.modules.method_application(&loaded, &key).is_none(),
                "bounded index eviction must not depend on an old handle's liveness"
            );
            runtime.collect_garbage().unwrap();
            assert!(
                unrooted.iter().all(|record| record.upgrade().is_none()),
                "pure application indexes cannot keep expired providers alive"
            );
            drop((retained, value));
            runtime.collect_garbage().unwrap();
            let record = runtime
                .application_record_in(
                    &[MetadataSource::AdmittedApplication(&computation)],
                    application,
                )
                .unwrap();
            assert!(matches!(record.as_ref(), ApplicationRecord::Semantic(_)));
            drop(record);
            let result = stack
                .finish_return(runtime, ReturnValue::scalar(ValueType::I32, 7))
                .unwrap();
            let ReturnAction::Complete(result) = result else {
                panic!("root generic return");
            };
            assert_eq!(
                stack.retain_result(runtime, result).unwrap().as_i32(),
                Some(7)
            );
            assert_eq!(
                runtime.application_record(&computation).unwrap_err().kind(),
                RuntimeErrorKind::ModuleValidation
            );
            drop(computation);
            runtime.collect_garbage().unwrap();
            assert!(expired.upgrade().is_none());
            assert_eq!(runtime.gc_stats().objects, baseline.objects);
            assert_eq!(
                runtime.gc_stats().root_registrations,
                baseline.root_registrations
            );
            Ok(())
        })
        .unwrap();
}

#[rstest]
#[case::abandoned(false)]
#[case::published(true)]
fn retained_environments_reject_abandoned_transitive_witness_providers(#[case] publish: bool) {
    let (mut runtime, loaded_program, loaded, table) = generic_fixture();
    let value = runtime
        .make_interface(&loaded, table, &RootedValue::from(1i32))
        .unwrap();
    let candidate = runtime
        .stage_reload_verified_program(&loaded_program, None, loaded.verified_program().clone())
        .unwrap();
    let supplier = candidate.module().clone();
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let (supplier_value, group) = {
        let mut session = runtime.begin_candidate_initialization(&candidate).unwrap();
        let runtime = session.runtime_mut();
        let value = runtime
            .make_interface(&supplier, table, &RootedValue::from(2i32))
            .unwrap();
        let description = runtime.interface_description(&value).unwrap();
        let group = runtime
            .bind_table_operations(
                execution,
                &description.receiver_table,
                &[MetadataSource::RootedInterface(&value)],
            )
            .unwrap();
        (value, group)
    };
    let mut operations = OperationBindings::default();
    operations.receiver(group.record()).unwrap();
    let arguments = runtime
        .resolve_type_arguments(&loaded, &[Ty::Builtin(BuiltinType::I32)])
        .unwrap();
    let interface = runtime
        .interface_description(&value)
        .unwrap()
        .interface_type()
        .clone();
    let method = runtime
        .prepare_interface_method_slot(
            &value,
            &interface,
            0,
            MethodApplicationInput {
                arguments: &arguments,
                operations,
                constructions: Default::default(),
                sources: &[MetadataSource::AdmittedOperations(&group)],
            },
        )
        .unwrap();
    let environment = semantic_environment(&mut runtime, &method);
    let admitted = runtime
        .copy_environment_in(execution, &environment, &method.sources().unwrap())
        .unwrap()
        .unwrap();
    let root = runtime.retain_environment(&admitted).unwrap();
    let pending = runtime
        .environment_snapshot(&admitted)
        .unwrap()
        .record
        .as_ref()
        .clone();
    runtime.validate_environment(&admitted).unwrap();
    let expired = Arc::downgrade(method.invocation.application.as_ref().unwrap().record());
    drop((method, supplier_value, group));
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    if publish {
        runtime.publish_staged_reload(candidate).unwrap();
    } else {
        drop(candidate);
    }
    // Physical retention cannot authorize an abandoned transitive program.
    let sources = [MetadataSource::HostEnvironment(&root)];
    let validation = runtime.validate_environment_in(&sources, &environment);
    if publish {
        validation.unwrap();
    } else {
        assert_eq!(
            validation.unwrap_err().kind(),
            RuntimeErrorKind::StaleHandle
        );
    }
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let before = runtime.gc_stats();
    let allocation = runtime.alloc_environment(execution, pending, &sources);
    if publish {
        runtime.validate_environment(&allocation.unwrap()).unwrap();
    } else {
        assert_eq!(
            allocation.unwrap_err().kind(),
            RuntimeErrorKind::StaleHandle
        );
        assert_eq!(runtime.gc_stats(), before);
    }
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(
        expired.upgrade().is_none(),
        "an index cannot retain a semantic application"
    );
    assert!(!runtime.is_quarantined());
    drop((root, value));
    runtime.collect_garbage().unwrap();
}

#[test]
fn witness_preparation_reuses_checked_selections_and_retires_with_its_program() {
    let (mut runtime, program, loaded) = load_source(
        include_str!("../../../tests/fixtures/metadata/interface-witnesses.kgr"),
        None,
    );
    let (closed_site, first_call) = loaded
        .bytecode
        .functions
        .iter()
        .find_map(|function| {
            let links = runtime
                .modules
                .linked_function(&loaded, function.id, None)?;
            (0..loaded.execution().functions[function.id.index()]
                .calls
                .len())
                .find_map(|ordinal| {
                    let call = links.call(ordinal)?;
                    (!call.operations.is_empty()).then(|| {
                        (
                            InterfaceCallSite {
                                function: function.id,
                                ordinal,
                            },
                            call.clone(),
                        )
                    })
                })
        })
        .unwrap();
    let (site, parameters) = loaded
        .bytecode
        .functions
        .iter()
        .find_map(|function| {
            let generic = function.metadata.semantic.generic.as_ref()?;
            let links = runtime
                .modules
                .linked_function(&loaded, function.id, None)?;
            (0..loaded.execution().functions[function.id.index()]
                .calls
                .len())
                .find_map(|ordinal| match links.call_contract(ordinal) {
                    Some(LinkedCall::Interface { contract, .. }) if contract.receiver.is_some() => {
                        Some((
                            InterfaceCallSite {
                                function: function.id,
                                ordinal,
                            },
                            generic.parameters.clone(),
                        ))
                    }
                    _ => None,
                })
        })
        .unwrap();
    let second = runtime
        .modules
        .linked_function(&loaded, closed_site.function, None)
        .unwrap()
        .call(closed_site.ordinal)
        .unwrap()
        .clone();
    assert!(Arc::ptr_eq(&first_call, &second));
    let lexical = MetadataSource::Revision {
        owner: &loaded,
        source: RevisionSource::Current,
    };
    assert!(
        runtime
            .interface_call_is_protected(&[lexical], &first_call)
            .unwrap()
    );
    runtime.collect_garbage().unwrap();
    assert!(
        runtime
            .interface_call_is_protected(&[lexical], &first_call)
            .unwrap()
    );
    let arguments = runtime
        .resolve_type_arguments(&loaded, &[Ty::Builtin(BuiltinType::I32)])
        .unwrap();
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let mut record =
        EnvironmentRecord::new(loaded.definitions(), parameters.clone(), arguments.clone())
            .unwrap();
    record.extend_operations(first_call.operations.clone());
    record.set_constructions(first_call.arguments.constructions().clone());
    let environment = runtime
        .alloc_environment(execution, record, &[lexical])
        .unwrap();
    let root = runtime.retain_environment(&environment).unwrap();
    let mut protected = vec![lexical, MetadataSource::AdmittedEnvironment(&environment)];
    let first = runtime
        .prepare_scoped_interface_call(
            execution,
            &loaded,
            environment.facts().clone(),
            site,
            &mut protected,
        )
        .unwrap();
    let repeated = runtime
        .prepare_scoped_interface_call(
            execution,
            &loaded,
            environment.facts().clone(),
            site,
            &mut protected,
        )
        .unwrap();
    assert!(Arc::ptr_eq(&first, &repeated));
    assert!(first.operation.is_some());
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    assert_eq!(
        runtime
            .environment_snapshot(&environment)
            .err()
            .expect("retired environment source")
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    // Exceed the single 128-entry scope index. Each execution retires independently;
    // bounded cache facts must not retain its actual environment record.
    let mut expired = Vec::new();
    for _ in 0..128 {
        let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
        let mut record =
            EnvironmentRecord::new(loaded.definitions(), parameters.clone(), arguments.clone())
                .unwrap();
        record.extend_operations(first_call.operations.clone());
        record.set_constructions(first_call.arguments.constructions().clone());
        let environment = runtime
            .alloc_environment(execution, record, &[lexical])
            .unwrap();
        let snapshot = runtime.environment_snapshot(&environment).unwrap();
        expired.push(Arc::downgrade(&snapshot.record));
        drop(snapshot);
        let mut sources = vec![lexical, MetadataSource::AdmittedEnvironment(&environment)];
        let prepared = runtime
            .prepare_scoped_interface_call(
                execution,
                &loaded,
                environment.facts().clone(),
                site,
                &mut sources,
            )
            .unwrap();
        assert_eq!(prepared.operation, first.operation);
        runtime
            .memory
            .retire_execution(&runtime.resources, execution)
            .unwrap();
    }
    assert!(
        runtime
            .modules
            .interface_call(&loaded, environment.facts().id, site)
            .is_none()
    );
    runtime.collect_garbage().unwrap();
    assert!(expired.iter().all(|record| record.upgrade().is_none()));
    assert!(
        runtime
            .interface_call_is_protected(&[MetadataSource::HostEnvironment(&root)], &first)
            .unwrap()
    );
    let candidate = runtime
        .stage_reload_verified_program(&program, None, loaded.verified_program().clone())
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    let current = runtime.select_program(&program).unwrap();
    assert_ne!(current.program_identity(), loaded.program_identity());
    runtime.collect_garbage().unwrap();
    assert!(
        runtime
            .interface_call_is_protected(&[MetadataSource::HostEnvironment(&root)], &first)
            .unwrap()
    );
    drop(root);
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules.loaded(loaded.key()).is_none());
    assert!(runtime.modules.loaded(current.key()).is_some());
    let current_call = runtime
        .modules
        .linked_function(&current, closed_site.function, None)
        .unwrap()
        .call(closed_site.ordinal)
        .unwrap()
        .clone();
    assert_ne!(
        first_call.operations.identity(),
        current_call.operations.identity()
    );
    assert!(
        runtime
            .interface_call_is_protected(
                &[MetadataSource::Revision {
                    owner: &current,
                    source: RevisionSource::Current
                }],
                &current_call
            )
            .unwrap()
    );
    assert!(!runtime.is_quarantined());
}
