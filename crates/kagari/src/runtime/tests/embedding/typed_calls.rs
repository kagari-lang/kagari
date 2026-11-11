#![cfg(feature = "source")]

use crate::{
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::Program,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_compiler::{
    bytecode::lower_program_to_bytecode,
    source::{program::lower_program_to_mir, source_package::build_source_package},
};
use kagari_contract::source::ExecutableOrigin;
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_runtime::reload::ReloadValidationError;
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::ExecutionFrame,
    native::{
        assembly::{ModuleDefinition, ModuleSetBuilder},
        builder::ModuleBuilder,
        declarations::MethodDecl,
        function_handle::Function,
        interfaces::Interface,
        module::NativeModule,
        objects::Object,
        storage::NativeStorage,
        types::Type,
    },
    session::{ExecutionEvent, ExecutionObserver},
    source::{SourceQuery, SourceUnavailableReason},
};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib as stdlib;
use kagari_types::language::{self, Protocol};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};

fn prepare_typed_program(source: &str, providers: &[NativeModule]) -> Program {
    // Exercise SDK admission and source observations under explicitly authored providers.
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("typed-calls.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        providers
            .iter()
            .map(|provider| Arc::new(provider.to_declaration()))
            .collect(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &Default::default()).unwrap();
    let package = build_source_package(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &bytecode,
        &Default::default(),
    )
    .unwrap();
    let artifact =
        KbcArtifact::from_verified(bytecode, Default::default(), &Default::default()).unwrap();
    let artifact =
        KbcArtifact::with_source_package(artifact, &package, &Default::default()).unwrap();
    Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap()
}

fn future_provider() -> NativeModule {
    let mut builder = ModuleBuilder::new("fixture::typed", &Default::default()).unwrap();
    let mut future = builder.define_type("Future").unwrap();
    future.type_parameter("T").unwrap();
    future.native_storage(NativeStorage::future()).unwrap();
    future.finish().unwrap();
    builder.finish().unwrap()
}

#[test]
fn existing_typed_handles_select_current_implementations_and_keep_closure_bodies() {
    let source = include_str!("../../../../tests/fixtures/embedding/typed/current-handles-v1.kgr");
    let first = prepare_typed_program(source, &[]);
    let second = prepare_typed_program(
        include_str!("../../../../tests/fixtures/embedding/typed/current-handles-v2.kgr"),
        &[],
    );
    let mut runtime = {
        let mut runtime = Runtime::default();
        runtime.set_default_context(Default::default());
        runtime
    };
    let program = runtime.load_program(&first, Default::default()).unwrap();
    let function = runtime
        .bind_function::<(), i32>(&program.root(), "selected")
        .unwrap();
    let named_factory = runtime
        .bind_function::<(), Function<(), i32>>(&program.root(), "named")
        .unwrap();
    let closure_factory = runtime
        .bind_function::<(), Function<(), i32>>(&program.root(), "closure")
        .unwrap();
    let make = runtime
        .bind_function::<(), Object>(&program.root(), "make")
        .unwrap();
    let view = runtime
        .bind_function::<(), Interface>(&program.root(), "view")
        .unwrap();
    let named = runtime.call(&named_factory, ()).unwrap();
    assert!(named.call_route().is_some());
    let closure = runtime.call(&closure_factory, ()).unwrap();
    let object = runtime.call(&make, ()).unwrap();
    let interface = runtime.call(&view, ()).unwrap();
    let method = runtime
        .bind_method::<(), i32>(object.object_type(), "read")
        .unwrap();
    let interface_method = interface
        .member(&runtime, "read")
        .and_then(|member| runtime.bind_interface_method::<(), i32>(&member))
        .unwrap();
    assert_eq!(runtime.call(&function, ()).unwrap(), 10);
    runtime
        .program(&program)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| base.prepare_replacement(&second, &Default::default()))
        .and_then(|candidate| runtime.replace_program(&program, &candidate, Default::default()))
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.call(&function, ()).unwrap(), 20);
    let rebound = runtime
        .bind_function::<(), i32>(&program.root(), "selected")
        .unwrap();
    assert_eq!(runtime.call(&rebound, ()).unwrap(), 7);
    assert_eq!(runtime.call(&named, ()).unwrap(), 20);
    assert_eq!(runtime.call(&closure, ()).unwrap(), 120);
    let fresh_closure = runtime.call(&closure_factory, ()).unwrap();
    assert_eq!(runtime.call(&fresh_closure, ()).unwrap(), 220);
    let current = runtime.select_program(&program).unwrap();
    runtime
        .with_context(&current, &Default::default(), |cx| {
            assert_eq!(object.call(cx, &method, ())?, 21);
            assert_eq!(interface.call(cx, &interface_method, ())?, 21);
            Ok(())
        })
        .unwrap();
    let fresh = runtime.call(&make, ()).unwrap();
    runtime
        .with_context(&current, &Default::default(), |cx| {
            assert_eq!(fresh.call(cx, &method, ())?, 21);
            Ok(())
        })
        .unwrap();
    let foreign = {
        let mut runtime = Runtime::default();
        runtime.set_default_context(Default::default());
        runtime
    };
    assert!(foreign.call(&function, ()).is_err());
}

#[test]
fn async_typed_admission_retains_selected_factory_before_any_poll() {
    let source = include_str!("../../../../tests/fixtures/embedding/typed/async-admission-v1.kgr");
    let providers = [future_provider()];
    let first = prepare_typed_program(source, &providers);
    let second = prepare_typed_program(
        include_str!("../../../../tests/fixtures/embedding/typed/async-admission-v2.kgr"),
        &providers,
    );
    let mut core = Runtime::default();
    NativeModule::install_all(&providers, &mut core).unwrap();
    let mut runtime = {
        let mut runtime = core;
        runtime.set_default_context(Default::default());
        runtime
    };
    let program = runtime.load_program(&first, Default::default()).unwrap();
    let function = runtime
        .bind_async_function::<(), i32>(&program.root(), "run")
        .unwrap();
    let mut admitted = runtime.submit_async(&function, ()).unwrap();
    runtime
        .program(&program)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| base.prepare_replacement(&second, &Default::default()))
        .and_then(|candidate| runtime.replace_program(&program, &candidate, Default::default()))
        .unwrap();
    runtime.collect_garbage().unwrap();
    let mut current = runtime.submit_async(&function, ()).unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    let mut results = (None, None);
    for _ in 0..100 {
        let _ = runtime.poll(&mut cx);
        if results.0.is_none()
            && let Poll::Ready(result) = Pin::new(&mut admitted).poll(&mut cx)
        {
            results.0 = Some(result.unwrap());
        }
        if results.1.is_none()
            && let Poll::Ready(result) = Pin::new(&mut current).poll(&mut cx)
        {
            results.1 = Some(result.unwrap());
        }
        if results.0.is_some() && results.1.is_some() {
            break;
        }
    }
    assert_eq!(results, (Some(12), Some(102)));
}

#[test]
fn interface_source_targets_distinguish_unbound_members_from_observed_implementations() {
    let source = include_str!("../../../../tests/fixtures/embedding/typed/interface-source-v1.kgr");
    let providers = [future_provider()];
    let first = prepare_typed_program(source, &providers);
    let second = prepare_typed_program(
        include_str!("../../../../tests/fixtures/embedding/typed/interface-source-v2.kgr"),
        &providers,
    );
    let mut core = Runtime::default();
    NativeModule::install_all(&providers, &mut core).unwrap();
    let mut runtime = {
        let mut runtime = core;
        runtime.set_default_context(Default::default());
        runtime
    };
    let program = runtime.load_program(&first, Default::default()).unwrap();
    let old_key = runtime.select_program(&program).unwrap().key();
    let integer = runtime
        .bind_function::<(), Interface>(&program.root(), "integer")
        .unwrap();
    let boolean = runtime
        .bind_function::<(), Interface>(&program.root(), "boolean")
        .unwrap();
    let integer = runtime.call(&integer, ()).unwrap();
    let boolean = runtime.call(&boolean, ()).unwrap();
    let method = integer
        .member(&runtime, "read")
        .and_then(|member| runtime.bind_interface_method::<(), i32>(&member))
        .unwrap();
    let later = integer
        .member(&runtime, "later")
        .and_then(|member| runtime.bind_async_interface_method::<(), i32>(&member))
        .unwrap();
    let unbound = method.source_target();
    let unbound_async = later.source_target();
    for (target, name) in [(&unbound, "read"), (&unbound_async, "later")] {
        let SourceQuery::Unavailable(info) = runtime.function_source(target).unwrap() else {
            panic!("an unbound member must not claim a default or concrete implementation");
        };
        assert_eq!(info.reason, SourceUnavailableReason::NoObservedBody);
        let declaration = info.declaration.unwrap();
        let names: Vec<_> = declaration
            .path
            .iter()
            .map(|segment| segment.name.as_str())
            .collect();
        assert_eq!(names, ["Inspect", name]);
        assert!(info.application.is_none());
        assert!(info.target.is_none());
        assert!(info.executable.is_none());
    }
    let rooted = integer.prepare_call(&runtime, &method).unwrap();
    let target = rooted.source_target(&runtime).unwrap();
    let other = boolean.prepare_call(&runtime, &method).unwrap();
    let other_target = other.source_target(&runtime).unwrap();
    let history = runtime.source_snapshot(&program).unwrap();
    let SourceQuery::Available(old) = runtime.function_source(&target).unwrap() else {
        panic!("retained source package must identify the selected implementation");
    };
    assert_eq!(old.text, "fn read(self) -> i32 { 7 }");
    let SourceQuery::Available(other_source) = runtime.function_source(&other_target).unwrap()
    else {
        panic!("second receiver implementation source");
    };
    assert_eq!(other_source.text, "fn read(self) -> i32 { 9 }");
    assert_ne!(old.target, other_source.target);
    runtime
        .program(&program)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| base.prepare_replacement(&second, &Default::default()))
        .and_then(|candidate| runtime.replace_program(&program, &candidate, Default::default()))
        .unwrap();
    let SourceQuery::Available(current) = runtime.function_source(&target).unwrap() else {
        panic!("retained ordinary method target must observe current code");
    };
    assert_eq!(current.text, "fn read(self) -> i32 { 17 }");
    assert_ne!(current.publication, old.publication);
    drop((rooted, other, integer, boolean, method, later));
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old_key)
    );
    let SourceQuery::Available(after_release) = runtime.function_source(&target).unwrap() else {
        panic!("current route must not require its original receiver or executable");
    };
    assert_eq!(after_release.text, current.text);
    assert_eq!(after_release.publication, current.publication);
    let SourceQuery::Available(other_current) = runtime.function_source(&other_target).unwrap()
    else {
        panic!("route must preserve the originally bound receiver's implementation");
    };
    assert_eq!(other_current.text, other_source.text);
    assert_eq!(other_current.publication, current.publication);
    let SourceQuery::Available(historical) = history.function(&old.declaration) else {
        panic!("explicit source snapshot must preserve history after executable reclamation");
    };
    assert_eq!(historical.text, old.text);
    assert_eq!(historical.publication, old.publication);
    assert!(
        matches!(runtime.function_source(&unbound).unwrap(), SourceQuery::Unavailable(info)
        if info.reason == SourceUnavailableReason::NoObservedBody && info.declaration.is_some())
    );
    assert!(Runtime::default().function_source(&target).is_err());
    assert!(Runtime::default().function_source(&unbound_async).is_err());
}

#[test]
fn interface_source_targets_intrinsic_bridges_survive_executable_reclamation() {
    let mut providers = ModuleSetBuilder::new(&Default::default()).unwrap();
    providers
        .add(ModuleDefinition::new(Protocol::Hash.module(), |module| {
            module.package_alias("core");
            let hash = module.reference_trait(language::identity(Protocol::Hash), 0)?;
            let method = module
                .trait_method(hash.id(), MethodDecl::instance("hash").returns(Type::i64()))?;
            module.define_trait("Hash", 0, |record, _| {
                record.methods.push(method);
                Ok(())
            })?;
            Ok(())
        }))
        .unwrap();
    let providers = providers.finish().unwrap().into_modules().unwrap();
    let source = include_str!("../../../../tests/fixtures/embedding/typed/intrinsic-source-v1.kgr");
    let first = prepare_typed_program(source, &providers);
    let second = prepare_typed_program(
        include_str!("../../../../tests/fixtures/embedding/typed/intrinsic-source-v2.kgr"),
        &providers,
    );
    let mut core = Runtime::default();
    NativeModule::install_all(&providers, &mut core).unwrap();
    let mut runtime = {
        let mut runtime = core;
        runtime.set_default_context(Default::default());
        runtime
    };
    let program = runtime.load_program(&first, Default::default()).unwrap();
    let old_key = runtime.select_program(&program).unwrap().key();
    let mut observations = Vec::new();
    for name in ["primitive", "tuple"] {
        let factory = runtime
            .bind_function::<(), Interface>(&program.root(), name)
            .unwrap();
        let receiver = runtime.call(&factory, ()).unwrap();
        let method = receiver
            .member(&runtime, "hash")
            .and_then(|member| runtime.bind_interface_method::<(), i64>(&member))
            .unwrap();
        let rooted = receiver.prepare_call(&runtime, &method).unwrap();
        let target = rooted.source_target(&runtime).unwrap();
        let SourceQuery::Unavailable(before) = runtime.function_source(&target).unwrap() else {
            panic!("intrinsic bridge must not fabricate authored text");
        };
        assert_eq!(before.reason, SourceUnavailableReason::NativeOrigin);
        assert_eq!(
            before.executable.as_ref().unwrap().origin,
            ExecutableOrigin::Intrinsic
        );
        assert!(before.application.is_some());
        assert!(before.target.is_some());
        assert!(before.native_binding.is_none());
        observations.push((target, before));
    }
    runtime
        .program(&program)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| base.prepare_replacement(&second, &Default::default()))
        .and_then(|candidate| runtime.replace_program(&program, &candidate, Default::default()))
        .unwrap();
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old_key)
    );
    for (target, before) in observations {
        let SourceQuery::Unavailable(after) = runtime.function_source(&target).unwrap() else {
            panic!("intrinsic bridge diagnostic must survive executable reclamation");
        };
        assert_eq!(after.reason, SourceUnavailableReason::NativeOrigin);
        assert_eq!(after.publication, before.publication);
        assert_eq!(after.target, before.target);
        assert_eq!(after.application, before.application);
        assert_eq!(after.executable, before.executable);
        assert_eq!(after.native_binding, before.native_binding);
    }
}

#[derive(Debug)]
struct Observer(Arc<AtomicUsize>);

impl ExecutionObserver for Observer {
    fn observe(
        &mut self,
        _: &Runtime,
        _: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[test]
fn bound_calls_use_the_sdk_execution_context_and_return_retained_values() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://calls.kgr",
                r#"
        pub struct Player { pub var hp: i32 }
        impl Player {
            pub fn damage(self, amount: i32) -> i32 { self.hp = self.hp - amount; self.hp }
        }
        pub fn damage_evidence() -> i32 { make().damage(1) }
        pub fn make() -> Player { Player { hp: 42 } }
        pub fn make_callback() -> fn() -> i32 { val data = [42]; || data[0] }
    "#,
            ),
            Default::default(),
        )
        .unwrap();
    let program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let _selected_loaded = runtime.select_program(&loaded).unwrap();
    let make = runtime
        .bind_function::<(), Object>(&loaded.root(), "make")
        .unwrap();
    let make_callback = runtime
        .bind_function::<(), Function<(), i32>>(&loaded.root(), "make_callback")
        .unwrap();
    let prepared = make.prepare_call(&runtime).unwrap();
    assert!(
        runtime
            .with_context(prepared.owner(), &context, |cx| {
                prepared.call_with_options::<(), bool>(cx, (), Default::default())
            })
            .is_err()
    );
    drop(prepared);
    let player = runtime.call_with_context(&make, (), &context).unwrap();
    let callback = runtime
        .call_with_context(&make_callback, (), &context)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.call_with_context(&callback, (), &context).unwrap(),
        42
    );
    let hp = runtime
        .bind_field::<i32>(player.object_type(), "hp")
        .unwrap();
    let damage = runtime
        .bind_method::<(i32,), i32>(player.object_type(), "damage")
        .unwrap();
    let observations = Arc::new(AtomicUsize::new(0));
    runtime
        .set_execution_observer(Observer(observations.clone()))
        .unwrap();
    let remaining = runtime
        .with_context(&_selected_loaded, &context, |cx| {
            assert_eq!(player.get(cx, &hp)?, 42);
            player.set(cx, &hp, 100)?;
            player.call(cx, &damage, (10,))
        })
        .unwrap();
    assert_eq!(remaining, 90);
    assert!(observations.load(Ordering::Relaxed) > 0);
    let cancelled = ExecutionContext::default();
    cancelled.cancellation.cancel();
    assert!(runtime.call_with_context(&make, (), &cancelled).is_err());
    assert!(
        runtime
            .with_context(&_selected_loaded, &cancelled, |cx| player.set(cx, &hp, 0))
            .is_err()
    );
    assert_eq!(
        runtime
            .with_context(&_selected_loaded, &context, |cx| player.get(cx, &hp))
            .unwrap(),
        90
    );
    let unsupported = ExecutionContext {
        jit_policy: JitPolicy::Enabled,
        ..Default::default()
    };
    assert!(
        runtime
            .call_with_context(&callback, (), &unsupported)
            .is_err()
    );
    assert_eq!(
        runtime.call_with_context(&callback, (), &context).unwrap(),
        42
    );
    drop(callback);
    drop(player);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}
