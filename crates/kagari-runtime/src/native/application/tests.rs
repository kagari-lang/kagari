use crate::{
    Runtime,
    error::RuntimeErrorKind,
    execution::roots::{ExecutionRoots, SessionId},
    execution_metadata::{
        applications::{ApplicationId, ApplicationRecord},
        managed::sources::MetadataSource,
    },
    frame::types::{EnvironmentRecord, construction::ConstructionScope},
    module::managed::source::RevisionSource,
    native::{
        application::NativeApplication,
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        context::{CallableOwner, LinkedCallable, LinkedOperation, NativeCall, NativeEntry},
        conversion::arguments::ArgumentValues,
        declarations::FunctionDecl,
        types::Type,
    },
    session::ExecutionOptions,
    test_support::{bytecode::load_bytecode, native::program::free_function_program},
    value::{carriers::ScopedValue, place::ResultPlace, view::ValueView},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::sync::{Arc, Mutex, OnceLock};

#[test]
fn active_native_application_outlives_its_original_execution_without_host_roots() {
    // The callback retires an independent source, not its own Native suffix.
    // Host control contains only a checked execution ID, never a heap pointer.
    let original: Arc<Mutex<Option<(SessionId, ApplicationId, usize)>>> = Arc::default();
    let retire = original.clone();
    let mut builder =
        ModuleBuilder::new("test::application", &DeclarationCatalog::default()).unwrap();
    let probe = builder
        .define_function(FunctionDecl::new("probe").returns(Type::i64()))
        .unwrap();
    builder
        .bind(
            probe,
            NativeBinding::scoped(vec![], Codec::Exact(Type::i64()), move |call| {
                let (original, id, roots) = retire.lock().unwrap().take().unwrap();
                call.runtime
                    .memory
                    .retire_execution(&call.runtime.resources, original)
                    .unwrap();
                call.collect_garbage()?;
                let source = MetadataSource::Application(call.pending.application().unwrap());
                // The installed function is a real closed Native entry; the
                // explicit application isolates metadata admission/retirement.
                let application = call.runtime.application_record_in(&[source], id)?;
                let ApplicationRecord::Native(application) = application.as_ref() else {
                    panic!("native application")
                };
                let snapshot = call
                    .runtime
                    .environment_snapshot_in(&[source], &application.environment)?;
                assert!(snapshot.record.operations().is_empty());
                assert_eq!(
                    snapshot.record.constructions.programs()[0].key(),
                    call.owner().key()
                );
                let selected = application.function.selected[0]
                    .ready()
                    .unwrap()
                    .environment
                    .as_ref()
                    .unwrap();
                assert_ne!(selected.id, application.environment.id);
                let snapshot = call.runtime.environment_snapshot_in(&[source], selected)?;
                assert!(snapshot.record.operations().is_empty());
                assert!(snapshot.record.constructions.programs().is_empty());
                // The pure preparation cache was evicted before entry. Its old
                // facts cannot supply the actual environment held by this call.
                assert!(
                    call.runtime
                        .modules
                        .native_application(
                            call.owner(),
                            NativeImportId::new(0),
                            application.environment.id
                        )
                        .is_none()
                );
                assert_eq!(call.runtime.gc_stats().root_registrations, roots);
                call.set_result(ValueView::from(7i64))
            }),
        )
        .unwrap();
    let native = builder.finish().unwrap();
    let bytecode = free_function_program(&native.to_declaration());
    let mut runtime = Runtime::default();
    native.install(&mut runtime).unwrap();
    let program = load_bytecode(&mut runtime, "application", bytecode).unwrap();
    let owner = runtime.select_program(&program).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let mut record =
        EnvironmentRecord::new(&runtime.definition_context().snapshot(), vec![], vec![]).unwrap();
    record.set_constructions(ConstructionScope::new(&owner));
    let environment = runtime
        .alloc_environment(
            source,
            record,
            &[MetadataSource::Revision {
                owner: &owner,
                source: RevisionSource::Current,
            }],
        )
        .unwrap();
    let function = runtime
        .modules
        .native_binding(&owner, NativeImportId::new(0))
        .unwrap();
    let selected = runtime
        .alloc_environment(
            source,
            EnvironmentRecord::new(&runtime.definition_context().snapshot(), vec![], vec![])
                .unwrap(),
            &[],
        )
        .unwrap();
    let mut function = (*function).clone();
    function.selected = vec![LinkedOperation::Ready(LinkedCallable {
        owner: CallableOwner::Resolved(owner.clone()),
        target: CallableTarget::Native(NativeImportId::new(0)),
        params: Box::new([]),
        result: Ty::Builtin(BuiltinType::I64),
        primitive: None,
        environment: Some(selected.facts().clone()),
        scoped_signature: OnceLock::new(),
    })]
    .into_boxed_slice();
    let prepared = Arc::new(NativeApplication {
        owner: owner.clone(),
        environment: environment.facts().clone(),
        function,
    });
    let protected = [
        MetadataSource::AdmittedEnvironment(&environment),
        MetadataSource::AdmittedEnvironment(&selected),
    ];
    runtime
        .publish_native_application(&owner, NativeImportId::new(0), prepared.clone(), &protected)
        .unwrap();
    let application = runtime
        .alloc_application(
            source,
            ApplicationRecord::Native(prepared.clone()),
            &protected,
        )
        .unwrap();
    for _ in 0..160 {
        let temporary = runtime.memory.enter_execution(&runtime.resources).unwrap();
        let environment = runtime
            .alloc_environment(
                temporary,
                EnvironmentRecord::new(&runtime.definition_context().snapshot(), vec![], vec![])
                    .unwrap(),
                &[],
            )
            .unwrap();
        let mut function = prepared.function.clone();
        function.selected = Box::new([]);
        let other = Arc::new(NativeApplication {
            owner: owner.clone(),
            environment: environment.facts().clone(),
            function,
        });
        runtime
            .publish_native_application(
                &owner,
                NativeImportId::new(0),
                other,
                &[
                    MetadataSource::AdmittedEnvironment(&environment),
                    MetadataSource::Revision {
                        owner: &owner,
                        source: RevisionSource::Current,
                    },
                ],
            )
            .unwrap();
        runtime
            .memory
            .retire_execution(&runtime.resources, temporary)
            .unwrap();
    }
    *original.lock().unwrap() = Some((source, application.id(), baseline.root_registrations));
    let mut session = runtime
        .begin_execution(&owner, ExecutionOptions::default())
        .unwrap();
    let runtime = session.runtime_mut();
    let execution = runtime.resources.active_session().unwrap().id;
    let destination = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                slots.push_local(execution, ScopedValue::unit())
            })
        })
        .unwrap();
    let mut call = NativeCall::new(
        runtime,
        &owner,
        NativeEntry::Applied(&application),
        RevisionSource::Current,
        ArgumentValues::scalars(&[]),
        |_, _, _, _, _| panic!("closed callback has no script invocation"),
    )
    .unwrap();
    let result = call.invoke(ResultPlace::new(destination)).unwrap();
    result.admit(destination).unwrap();
    drop(call);
    assert_eq!(
        runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    slots.get(destination).map(|value| value.scalar_payload())
                })
            })
            .unwrap(),
        Some((ValueType::I64, 7))
    );
    assert_eq!(
        runtime.application_record(&application).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert!(!runtime.is_quarantined());
}
