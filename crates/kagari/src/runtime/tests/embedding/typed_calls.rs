#![cfg(feature = "source")]

use crate::{
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::PreparedProgram,
    runtime::Runtime as SdkRuntime,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_compiler::{
    bytecode::lower_program_to_bytecode,
    source::{program::lower_program_to_mir, source_package::build_source_package},
};
use kagari_contract::source::ExecutableOrigin;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionFrame, types::arguments::TypeArgument},
    native::{
        assembly::{ModuleDefinition, ModuleSetBuilder},
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        declarations::MethodDecl,
        function_handle::Function,
        interfaces::Interface,
        module::NativeModule,
        objects::Object,
        storage::NativeStorage,
        types::Type,
        value_handle::ScriptValue,
    },
    session::{ExecutionEvent, ExecutionObserver},
    source::{SourceQuery, SourceUnavailableReason},
};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
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

fn prepare_typed_program(source: &str, providers: &[NativeModule]) -> PreparedProgram {
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
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &Default::default()).unwrap();
    let package = build_source_package(&checked, &bytecode, &Default::default()).unwrap();
    let artifact =
        KbcArtifact::from_verified(bytecode, Default::default(), &Default::default()).unwrap();
    let artifact =
        KbcArtifact::with_source_package(artifact, &package, &Default::default()).unwrap();
    PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap()
}

struct EntryArgument {
    value: i32,
    registrations: usize,
    calls: Arc<AtomicUsize>,
}

impl KagariType for EntryArgument {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }
}

impl IntoKagari for EntryArgument {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<ScriptValue> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            cx.runtime()
                .modules()
                .retention_counts(cx.owner().key())
                .active_calls,
            self.registrations
        );
        // Converters may reenter and collect. Neither registry borrow may escape
        // the authority check, and the borrowed conversion adds no registration.
        let nested = cx
            .runtime()
            .begin_execution(cx.owner(), cx.runtime().execution_options())?;
        cx.runtime().collect_garbage()?;
        drop(nested);
        self.value.into_kagari(cx, expected)
    }
}

struct BorrowedResult(i32);

impl KagariType for BorrowedResult {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }
}

impl FromKagari for BorrowedResult {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ScriptValue,
    ) -> NativeResult<Self> {
        assert_eq!(
            cx.runtime()
                .modules()
                .retention_counts(cx.owner().key())
                .active_calls,
            1
        );
        cx.runtime().collect_garbage()?;
        i32::from_kagari(cx, expected, value).map(Self)
    }
}

#[test]
fn typed_entry_borrows_exact_execution_retention_but_direct_vm_keeps_owned_fallback() {
    let prepared = prepare_typed_program(
        r#"
        pub struct Player { pub var hp: i32 }
        pub fn make() -> Player { Player { hp: 100 } }
        pub fn divide(value: i32) -> i32 { 42 / value }
    "#,
        &[],
    );
    let mut runtime = SdkRuntime::new(Runtime::default(), Default::default());
    let program = runtime.load_program(&prepared, Default::default()).unwrap();
    let owner = runtime.runtime().select_program(&program).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let argument = |value, registrations| EntryArgument {
        value,
        registrations,
        calls: calls.clone(),
    };
    let result: BorrowedResult = runtime
        .execute_typed(&program, "divide", (argument(2, 1),), &Default::default())
        .unwrap();
    assert_eq!(result.0, 21);
    assert_eq!(
        runtime
            .runtime()
            .modules()
            .retention_counts(owner.key())
            .active_calls,
        0
    );
    assert_eq!(
        runtime
            .vm
            .execute_typed::<_, i32>(&owner, "divide", (argument(3, 2),))
            .unwrap(),
        14
    );
    let before = calls.load(Ordering::SeqCst);
    // Rust output mismatch precedes argument conversion on both entry paths.
    assert!(
        runtime
            .execute_typed::<_, String>(&program, "divide", (argument(2, 1),), &Default::default(),)
            .is_err()
    );
    assert!(
        runtime
            .vm
            .execute_typed::<_, String>(&owner, "divide", (argument(2, 2),))
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), before);
    assert!(
        runtime
            .execute_typed::<_, BorrowedResult>(
                &program,
                "divide",
                (argument(0, 1),),
                &Default::default(),
            )
            .is_err()
    );
    assert_eq!(
        runtime
            .runtime()
            .modules()
            .retention_counts(owner.key())
            .active_calls,
        0
    );
    let player: Object = runtime
        .execute_typed(&program, "make", (), &Default::default())
        .unwrap();
    runtime.runtime().collect_garbage().unwrap();
    let hp = runtime
        .runtime()
        .bind_field::<i32>(player.object_type(), "hp")
        .unwrap();
    runtime
        .with_context(&owner, &Default::default(), |cx| {
            assert_eq!(player.get(cx, &hp)?, 100);
            Ok(())
        })
        .unwrap();
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
    let mut runtime = SdkRuntime::new(Runtime::default(), Default::default());
    let program = runtime.load_program(&first, Default::default()).unwrap();
    let function = runtime
        .function::<(), i32>(&program.root(), "selected")
        .unwrap();
    let named_factory = runtime
        .function::<(), Function<(), i32>>(&program.root(), "named")
        .unwrap();
    let closure_factory = runtime
        .function::<(), Function<(), i32>>(&program.root(), "closure")
        .unwrap();
    let make = runtime
        .function::<(), Object>(&program.root(), "make")
        .unwrap();
    let view = runtime
        .function::<(), Interface>(&program.root(), "view")
        .unwrap();
    let named = runtime.call(&named_factory, ()).unwrap();
    assert!(named.call_route().is_some());
    let closure = runtime.call(&closure_factory, ()).unwrap();
    let object = runtime.call(&make, ()).unwrap();
    let interface = runtime.call(&view, ()).unwrap();
    let method = runtime
        .method::<(), i32>(object.object_type(), "read")
        .unwrap();
    let interface_method = runtime
        .interface_method::<(), i32>(&interface, "read")
        .unwrap();
    assert_eq!(runtime.call(&function, ()).unwrap(), 10);
    runtime
        .reload_program(&program, &second, Default::default())
        .unwrap();
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.call(&function, ()).unwrap(), 20);
    let rebound = runtime
        .function::<(), i32>(&program.root(), "selected")
        .unwrap();
    assert_eq!(runtime.call(&rebound, ()).unwrap(), 7);
    assert_eq!(runtime.call(&named, ()).unwrap(), 20);
    assert_eq!(runtime.call(&closure, ()).unwrap(), 120);
    let fresh_closure = runtime.call(&closure_factory, ()).unwrap();
    assert_eq!(runtime.call(&fresh_closure, ()).unwrap(), 220);
    let current = runtime.runtime().select_program(&program).unwrap();
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
    let mut foreign = SdkRuntime::new(Runtime::default(), Default::default());
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
    let mut runtime = SdkRuntime::new(core, Default::default());
    let program = runtime.load_program(&first, Default::default()).unwrap();
    let function = runtime
        .async_function::<(), i32>(&program.root(), "run")
        .unwrap();
    let mut admitted = runtime.submit_async(&function, ()).unwrap();
    runtime
        .reload_program(&program, &second, Default::default())
        .unwrap();
    runtime.runtime().collect_garbage().unwrap();
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
    let mut runtime = SdkRuntime::new(core, Default::default());
    let program = runtime.load_program(&first, Default::default()).unwrap();
    let old_key = runtime.runtime().select_program(&program).unwrap().key();
    let integer = runtime
        .function::<(), Interface>(&program.root(), "integer")
        .unwrap();
    let boolean = runtime
        .function::<(), Interface>(&program.root(), "boolean")
        .unwrap();
    let integer = runtime.call(&integer, ()).unwrap();
    let boolean = runtime.call(&boolean, ()).unwrap();
    let method = runtime
        .interface_method::<(), i32>(&integer, "read")
        .unwrap();
    let later = runtime
        .async_interface_method::<(), i32>(&integer, "later")
        .unwrap();
    let unbound = method.source_target();
    let unbound_async = later.source_target();
    for (target, name) in [(&unbound, "read"), (&unbound_async, "later")] {
        let SourceQuery::Unavailable(info) = runtime.runtime().function_source(target).unwrap()
        else {
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
    let rooted = integer.prepare_call(runtime.runtime(), &method).unwrap();
    let target = rooted.source_target(runtime.runtime()).unwrap();
    let other = boolean.prepare_call(runtime.runtime(), &method).unwrap();
    let other_target = other.source_target(runtime.runtime()).unwrap();
    let history = runtime.runtime().source_snapshot(&program).unwrap();
    let SourceQuery::Available(old) = runtime.runtime().function_source(&target).unwrap() else {
        panic!("retained source package must identify the selected implementation");
    };
    assert_eq!(old.text, "fn read(self) -> i32 { 7 }");
    let SourceQuery::Available(other_source) =
        runtime.runtime().function_source(&other_target).unwrap()
    else {
        panic!("second receiver implementation source");
    };
    assert_eq!(other_source.text, "fn read(self) -> i32 { 9 }");
    assert_ne!(old.target, other_source.target);
    runtime
        .reload_program(&program, &second, Default::default())
        .unwrap();
    let SourceQuery::Available(current) = runtime.runtime().function_source(&target).unwrap()
    else {
        panic!("retained ordinary method target must observe current code");
    };
    assert_eq!(current.text, "fn read(self) -> i32 { 17 }");
    assert_ne!(current.publication, old.publication);
    drop((rooted, other, integer, boolean, method, later));
    assert!(
        runtime
            .runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old_key)
    );
    let SourceQuery::Available(after_release) = runtime.runtime().function_source(&target).unwrap()
    else {
        panic!("current route must not require its original receiver or executable");
    };
    assert_eq!(after_release.text, current.text);
    assert_eq!(after_release.publication, current.publication);
    let SourceQuery::Available(other_current) =
        runtime.runtime().function_source(&other_target).unwrap()
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
        matches!(runtime.runtime().function_source(&unbound).unwrap(), SourceQuery::Unavailable(info)
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
    let mut runtime = SdkRuntime::new(core, Default::default());
    let program = runtime.load_program(&first, Default::default()).unwrap();
    let old_key = runtime.runtime().select_program(&program).unwrap().key();
    let mut observations = Vec::new();
    for name in ["primitive", "tuple"] {
        let factory = runtime
            .function::<(), Interface>(&program.root(), name)
            .unwrap();
        let receiver = runtime.call(&factory, ()).unwrap();
        let method = runtime
            .interface_method::<(), i64>(&receiver, "hash")
            .unwrap();
        let rooted = receiver.prepare_call(runtime.runtime(), &method).unwrap();
        let target = rooted.source_target(runtime.runtime()).unwrap();
        let SourceQuery::Unavailable(before) = runtime.runtime().function_source(&target).unwrap()
        else {
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
        .reload_program(&program, &second, Default::default())
        .unwrap();
    assert!(
        runtime
            .runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old_key)
    );
    for (target, before) in observations {
        let SourceQuery::Unavailable(after) = runtime.runtime().function_source(&target).unwrap()
        else {
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
    let engine = KagariEngine::builder().unwrap().build().unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://calls.kgr",
                r#"
        pub struct Position { pub val x: i32 }
        pub struct Player {
            pub var hp: i32,
            pub val position: Position,
            pub val name: String,
            val secret: i32,
        }
        impl Player {
            pub fn damage(self, amount: i32) -> i32 { self.hp = self.hp - amount; self.hp }
        }
        pub fn damage_evidence(player: Player) -> i32 { player.damage(1) }
        pub fn make(name: String) -> Player {
            Player { hp: 42, position: Position { x: 7 }, name, secret: 1 }
        }
        pub fn make_callback() -> fn() -> i32 { val data = [42]; || data[0] }
    "#,
            ),
            Default::default(),
        )
        .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let _selected_loaded = runtime.runtime().select_program(&loaded).unwrap();
    let make = runtime
        .runtime()
        .bind_function::<(String,), Object>(&loaded.root(), "make")
        .unwrap();
    let make_callback = runtime
        .runtime()
        .bind_function::<(), Function<(), i32>>(&loaded.root(), "make_callback")
        .unwrap();
    let prepared = make.prepare_call(runtime.runtime()).unwrap();
    assert!(
        runtime
            .with_context(prepared.owner(), &context, |cx| {
                prepared.call_with_options::<(String,), bool>(
                    cx,
                    ("Ada".to_owned(),),
                    Default::default(),
                )
            })
            .is_err()
    );
    drop(prepared);
    let player = runtime
        .call_with_context(&make, ("Ada".to_owned(),), &context)
        .unwrap();
    let name: String = runtime.get_field(&player, "name").unwrap();
    assert_eq!(name, "Ada");
    let position: Object = runtime.get_field(&player, "position").unwrap();
    assert!(runtime.get_field::<i32>(&player, "missing").is_err());
    assert!(runtime.get_field::<i32>(&player, "secret").is_err());
    assert!(runtime.get_field::<bool>(&player, "hp").is_err());
    let mut foreign = engine.runtime(Default::default());
    assert!(foreign.get_field::<i32>(&player, "hp").is_err());
    let callback = runtime
        .call_with_context(&make_callback, (), &context)
        .unwrap();
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(
        runtime.call_with_context(&callback, (), &context).unwrap(),
        42
    );
    let hp = runtime
        .runtime()
        .bind_field::<i32>(player.object_type(), "hp")
        .unwrap();
    let damage = runtime
        .runtime()
        .bind_method::<(i32,), i32>(player.object_type(), "damage")
        .unwrap();
    let observations = Arc::new(AtomicUsize::new(0));
    runtime
        .runtime()
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
    assert!(
        runtime
            .get_field_with_context::<i32>(&player, "hp", &cancelled)
            .is_err()
    );
    assert!(
        runtime
            .call_with_context(&make, ("Ada".to_owned(),), &cancelled)
            .is_err()
    );
    assert!(
        runtime
            .with_context(&_selected_loaded, &cancelled, |cx| player.set(cx, &hp, 0))
            .is_err()
    );
    assert_eq!(
        runtime
            .get_field_with_context::<i32>(&player, "hp", &context)
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
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.get_field::<i32>(&position, "x").unwrap(), 7);
    drop(position);
    assert_eq!(runtime.runtime().collect_garbage().unwrap().live_objects, 0);
}
