//! One observable suite for source, artifacts and the existing JIT/fallback.
//! No bytecode layouts or arena IDs appear in the fixture expectations.
use crate::program::native::NativeCompiler;
use crate::{
    context::{ExecutionContext, JitPolicy},
    engine::source::PreparationOptions,
    error::EmbeddingError,
    program::Compilation,
    runtime::Runtime,
};
use kagari_bytecode::artifact::{ArtifactCompatibility, KbcArtifact};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
        metadata::DefinitionMetadata,
    },
};
use kagari_runtime::execution::{
    ExecutionReport, JitExecutionStatus,
    native::{PreparedNativeEntry, PreparedNativeExecution},
    owned::DriveResult,
};
use kagari_runtime::{
    Runtime as CoreRuntime, RuntimeConfig,
    error::{RuntimeError, RuntimeErrorKind},
    frame::ExecutionFrame,
    host::{HostCallContext, HostError},
    module::{ExecutableModule, InstalledProgram, entry::ProgramEntry},
    native::{
        builder::ModuleBuilder, conversion::context::ConversionContext, host, module::NativeModule,
    },
    reload::ReloadValidationError,
    resource::RuntimeLimits,
    session::{ExecutionEvent, ExecutionObserver},
    value::{carriers::RootedValue, view::ValueView},
    value_semantics::script_equal,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, host::HostCallPolicy, logging, module::ModuleDecl},
};
use std::{
    cell::Cell,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

#[derive(Debug)]
struct CancelAt {
    seen: Cell<usize>,
    at: usize,
    token: CancellationToken,
}

impl ExecutionObserver for CancelAt {
    fn observe(
        &mut self,
        runtime: &CoreRuntime,
        event: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        if event == ExecutionEvent::BeforeInstruction {
            let seen = self.seen.get();
            self.seen.set(seen + 1);
            if seen == self.at {
                self.token.cancel();
            }
        }
        runtime.resources().poll_execution()
    }
}

#[derive(Clone, Copy, Debug)]
enum Route {
    Source,
    Artifact,
    Jit,
    ArtifactJit,
}

impl Route {
    const ALL: [Self; 4] = [Self::Source, Self::Artifact, Self::Jit, Self::ArtifactJit];
}

#[derive(Debug)]
enum Expected {
    Value(fn(&mut ConversionContext<'_>) -> RootedValue),
    Diagnostic(&'static str),
    IndexTrap,
    HostFailure,
    ScriptTrap(&'static str),
    Cancelled,
}

#[derive(Debug, PartialEq)]
struct HostCall {
    symbol: &'static str,
    args: Vec<String>,
}

/// The test host's append-only log has explicit commit records. Calls and
/// commits are distinct: rejected calls must not produce mutation records.
#[derive(Debug, PartialEq)]
struct Mutation {
    target: &'static str,
    previous_len: usize,
    appended: String,
}

#[derive(Debug, Default)]
struct RecordingHost {
    calls: Vec<HostCall>,
    mutations: Vec<Mutation>,
    log: Vec<String>,
}

struct PublishedReload<'a> {
    candidate: &'a Case<'a>,
    selected_result: fn(&mut ConversionContext<'_>) -> RootedValue,
    native_entry: &'a str,
}

struct Case<'a> {
    name: &'static str,
    source: &'a str,
    modules: &'a [(&'a str, &'a str)],
    published_reload: Option<PublishedReload<'a>>,
    expected: Expected,
    calls: &'static [&'static str],
    committed: &'static [&'static str],
    reject_call: Option<usize>,
    cancel_call: Option<usize>,
    repeat: usize,
    require_native: bool,
    cancel_at: Option<usize>,
    vector: Option<(&'static [i32], &'static [i32])>,
}

impl<'a> Case<'a> {
    fn new(name: &'static str, source: &'a str, expected: Expected) -> Self {
        Self {
            name,
            source,
            modules: &[],
            published_reload: None,
            expected,
            calls: &[],
            committed: &[],
            reject_call: None,
            cancel_call: None,
            repeat: 1,
            require_native: false,
            cancel_at: None,
            vector: None,
        }
    }

    fn effects(
        mut self,
        calls: &'static [&'static str],
        committed: &'static [&'static str],
    ) -> Self {
        self.calls = calls;
        self.committed = committed;
        self
    }

    fn vector(mut self, initial: &'static [i32], expected: &'static [i32]) -> Self {
        self.vector = Some((initial, expected));
        self
    }

    fn modules(mut self, modules: &'a [(&'a str, &'a str)]) -> Self {
        self.modules = modules;
        self
    }

    fn native(mut self) -> Self {
        self.require_native = true;
        self
    }
}

fn logging_module(
    entry: impl for<'call> Fn(
        &mut HostCallContext<'call>,
        &[ValueView<'call>],
    ) -> Result<ValueView<'call>, HostError>
    + Send
    + Sync
    + 'static,
) -> NativeModule {
    let mut builder = ModuleBuilder::from_declaration(
        logging::declaration().unwrap(),
        &kagari_stdlib::declarations::StandardDeclarations::default()
            .catalog()
            .unwrap(),
        Default::default(),
    )
    .unwrap();
    let log = builder.declared_function("log").unwrap();
    builder.bind(log, host::callback(entry)).unwrap();
    builder.finish().unwrap()
}

fn observe_vector() -> DefinitionMetadata<ModuleDecl> {
    let context = DefinitionContext::new().unwrap();
    let vector = kagari_types::ty::Ty::Array(Box::new(kagari_types::ty::Ty::Builtin(
        kagari_types::scalar::BuiltinType::I32,
    )));
    let identity = ModuleIdentity {
        package: PackageId("observe".into()),
        path: vec![],
    };
    let root = context.intern_root(&identity).unwrap();
    let binding = context
        .intern_child(root, DefinitionKind::Function, "vector", 0)
        .unwrap();
    let mut declaration = ModuleDecl::new(identity);
    declaration.functions.push(FnDecl {
        name: "vector".into(),
        host_policy: Some(HostCallPolicy {
            passing: vec![],
            effects: Default::default(),
        }),
        method_policy: Default::default(),
        implementation: CallableImplementation::Native(binding),
        generic_params: vec![],
        bounds: vec![],
        params: vec![],
        return_type: vector,
    });
    let definitions = declaration.collect_definitions(&context).unwrap();
    DefinitionMetadata::checked(definitions, declaration, &Default::default()).unwrap()
}

fn compile(case: &Case<'_>, publication: bool) -> Option<RoutePrograms> {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in case
        .modules
        .iter()
        .copied()
        .chain(std::iter::once(("root", case.source)))
    {
        let uri = format!("mem://{name}");
        sources
            .bind_module(
                &uri,
                ModuleIdentity {
                    package: PackageId("contract".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        root = Some(sources.set(&uri, text.into(), SourceLayer::Base).unwrap());
    }
    let engine = preparation::engine(case.vector.is_some());
    let checked = if publication {
        Ok(preparation::check_publication(
            sources.snapshot(),
            root.unwrap(),
        ))
    } else {
        engine.compile_snapshot(sources.snapshot(), root.unwrap(), &Default::default())
    };
    if let Expected::Diagnostic(code) = case.expected {
        let Err(EmbeddingError::Diagnostics { diagnostics }) = checked else {
            let outcome = checked.map(|_| "checked successfully");
            panic!("{}: expected diagnostic {code}, got {outcome:?}", case.name);
        };
        assert!(
            diagnostics.iter().any(|d| d.code == code),
            "{}: {diagnostics:?}",
            case.name
        );
        return None;
    }
    let checked = checked.unwrap_or_else(|error| panic!("{}: {error:?}", case.name));
    let source = engine
        .prepare(
            &checked,
            PreparationOptions {
                retain_native_input: true,
                ..Default::default()
            },
        )
        .unwrap();
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    let encoded =
        KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let encoded = Compilation::from_artifact(
        encoded,
        &ArtifactCompatibility::default(),
        &Default::default(),
    )
    .unwrap();
    Some(RoutePrograms { source, encoded })
}

struct RoutePrograms {
    source: Compilation,
    encoded: Compilation,
}

impl RoutePrograms {
    fn get(&self, route: Route) -> &Compilation {
        match route {
            Route::Source | Route::Jit => &self.source,
            Route::Artifact | Route::ArtifactJit => &self.encoded,
        }
    }
}

fn run_routes(case: &Case<'_>) {
    let Some(programs) = compile(case, case.published_reload.is_some()) else {
        return;
    };
    // Interpreter and JIT routes consume the same checked input. Validate direct
    // and serialized products separately; every route still gets a fresh runtime.
    let candidate = case
        .published_reload
        .as_ref()
        .map(|reload| compile(reload.candidate, true).expect("published candidate must compile"));
    for route in Route::ALL {
        run(
            case,
            route,
            programs.get(route),
            candidate.as_ref().map(|candidate| candidate.get(route)),
        );
    }
}

fn assert_outcome(
    runtime: &mut CoreRuntime,
    owner: &ExecutableModule,
    case: &Case<'_>,
    route: Route,
    attempt: usize,
    outcome: Result<ExecutionReport, RuntimeError>,
) {
    match (&case.expected, outcome.as_ref()) {
        (Expected::Value(expected), Ok(report)) => {
            let expected = expected(&mut ConversionContext::new(runtime, owner).unwrap());
            assert!(
                script_equal(runtime, &report.return_value, &expected).unwrap(),
                "{} ({route:?}, attempt {attempt})",
                case.name
            );
        }
        (Expected::IndexTrap, Err(error)) if error.kind() == RuntimeErrorKind::IndexOutOfBounds => {
        }
        (Expected::HostFailure, Err(error))
            if error.kind() == RuntimeErrorKind::HostCallFailure => {}
        (Expected::ScriptTrap(message), Err(error))
            if error.kind() == RuntimeErrorKind::ScriptTrap && error.to_string() == *message => {}
        (Expected::Cancelled, Err(error)) if error.kind() == RuntimeErrorKind::Cancelled => {}
        (expected, actual) => panic!(
            "{} ({route:?}, attempt {attempt}): expected {expected:?}, got {actual:?}",
            case.name
        ),
    }
}

fn execute_route(
    runtime: &mut Runtime,
    loaded: &InstalledProgram,
    case: &Case<'_>,
    route: Route,
    prepared: Option<&PreparedNativeEntry>,
) -> Result<ExecutionReport, RuntimeError> {
    match route {
        Route::Jit | Route::ArtifactJit => {
            let prepared = prepared.expect("native route is prepared before execution admission");
            if case.require_native {
                assert!(
                    matches!(prepared, PreparedNativeEntry::Native(_)),
                    "{} ({route:?}) must enter native code: {prepared:?}",
                    case.name
                );
            }
            // Both execution routes retain Core's typed reasons and traces.
            // Native preparation uses the SDK's trusted finalized handoff.
            let outcome = runtime.execute_prepared_module(
                &runtime.select_program(loaded)?,
                ProgramEntry::Main,
                prepared,
                runtime.execution_options(),
            );
            if case.require_native
                && let Ok(report) = &outcome
            {
                assert_eq!(
                    report.jit.as_ref().unwrap().status,
                    JitExecutionStatus::Native
                );
            }
            outcome
        }
        _ => {
            let context = runtime.default_context().clone();
            runtime.execute(loaded, ProgramEntry::Main, &[], &context)
        }
    }
}

fn prepare_native(
    runtime: &mut Runtime,
    compiler: &mut NativeCompiler,
    program: &Compilation,
    loaded: &InstalledProgram,
    entry: ProgramEntry<'_>,
    backend: &mut CraneliftBackend,
) -> PreparedNativeEntry {
    let code = compiler
        .prepare(program, entry, backend, &Default::default())
        .unwrap();
    code.install(runtime, loaded).unwrap()
}

fn run(
    case: &Case<'_>,
    route: Route,
    module: &Compilation,
    candidate_program: Option<&Compilation>,
) {
    let observe = observe_vector();
    let mut runtime = preparation::installation().runtime(RuntimeConfig {
        limits: RuntimeLimits {
            ..Default::default()
        },

        ..Default::default()
    });
    let host = Arc::new(Mutex::new(RecordingHost::default()));
    let capture = host.clone();
    let reject_call = case.reject_call;
    let cancel_call = case.cancel_call;
    let cancellation = CancellationToken::default();
    let cancel = cancellation.clone();
    logging_module(move |cx, args| {
        let [message] = args else {
            return Err(HostError::new("log requires one string"));
        };
        let message = cx
            .read_string(message, str::to_owned)
            .map_err(|error| HostError::new(error.to_string()))?;
        let mut state = capture.lock().unwrap();
        state.calls.push(HostCall {
            symbol: "host.log",
            args: vec![message.clone()],
        });
        if reject_call == Some(state.calls.len()) {
            return Err(HostError::new("test host rejected append"));
        }
        let previous_len = state.log.len();
        state.log.push(message.clone());
        state.mutations.push(Mutation {
            target: "host.log",
            previous_len,
            appended: message.clone(),
        });
        if cancel_call == Some(state.calls.len()) {
            cancel.cancel();
        }
        Ok(ValueView::unit())
    })
    .install(&mut runtime)
    .unwrap();
    let observed_vector = Arc::new(Mutex::new(None::<RootedValue>));
    if case.vector.is_some() {
        let capture_vector = observed_vector.clone();
        let capture_host = host.clone();
        let mut builder = ModuleBuilder::from_declaration(
            observe,
            &StandardDeclarations::default().catalog().unwrap(),
            Default::default(),
        )
        .unwrap();
        let vector = builder.declared_function("vector").unwrap();
        builder
            .bind(
                vector,
                host::callback(move |cx, _| {
                    capture_host.lock().unwrap().calls.push(HostCall {
                        symbol: "observe.vector",
                        args: vec![],
                    });
                    let root = capture_vector
                        .lock()
                        .unwrap()
                        .as_ref()
                        .cloned()
                        .ok_or_else(|| HostError::new("array fixture is not initialized"))?;
                    cx.import(&root)
                        .map_err(|error| HostError::new(error.to_string()))
                }),
            )
            .unwrap();
        builder.finish().unwrap().install(&mut runtime).unwrap();
    }
    let loaded = runtime
        .load_verified_program(case.name, module.clone().into_program().into_verified())
        .unwrap_or_else(|error| panic!("{} ({route:?}): {error}", case.name));
    let selected = runtime.select_program(&loaded).unwrap();
    let retained = case.vector.map(|(initial, _)| {
        let rooted = runtime
            .alloc_array(
                &selected,
                kagari_types::ty::Ty::Builtin(kagari_types::scalar::BuiltinType::I32),
                &initial
                    .iter()
                    .copied()
                    .map(RootedValue::from)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        *observed_vector.lock().unwrap() = Some(rooted.clone());
        rooted
    });
    if let Some(at) = case.cancel_at {
        runtime
            .set_execution_observer(CancelAt {
                seen: Cell::new(0),
                at,
                token: cancellation.clone(),
            })
            .unwrap();
    }
    let context = ExecutionContext {
        jit_policy: match route {
            Route::Jit | Route::ArtifactJit => JitPolicy::Enabled,
            Route::Source | Route::Artifact => JitPolicy::Disabled,
        },
        ..Default::default()
    };
    let mut vm = {
        let mut runtime = runtime;
        runtime.set_default_context(context);
        runtime
    };
    let mut compiler = NativeCompiler::default();
    let mut backend = CraneliftBackend::for_host().unwrap();
    let prepared = matches!(route, Route::Jit | Route::ArtifactJit).then(|| {
        prepare_native(
            &mut vm,
            &mut compiler,
            module,
            &loaded,
            ProgramEntry::Main,
            &mut backend,
        )
    });
    let old_native = case.published_reload.as_ref().and_then(|reload| {
        matches!(route, Route::Jit | Route::ArtifactJit).then(|| {
            [
                ProgramEntry::Main,
                ProgramEntry::Export(reload.native_entry),
            ]
            .map(|entry| {
                let prepared =
                    prepare_native(&mut vm, &mut compiler, module, &loaded, entry, &mut backend);
                if matches!(entry, ProgramEntry::Export(_)) {
                    assert!(
                        matches!(prepared, PreparedNativeEntry::Native(_)),
                        "{} ({route:?}): dependency must be native: {prepared:?}",
                        case.name
                    );
                }
                (entry, prepared)
            })
        })
    });
    vm.collect_garbage().unwrap();
    let retained_roots = vm.gc_stats().root_registrations;
    let run = |runtime: &mut Runtime| {
        if case.cancel_at.is_some() {
            runtime.attach_execution_observer().unwrap();
        }
        for attempt in 0..case.repeat {
            let outcome = execute_route(runtime, &loaded, case, route, prepared.as_ref());
            assert_outcome(runtime, &selected, case, route, attempt, outcome);
        }
        assert_eq!(
            runtime.resources().counters().current_call_depth,
            0,
            "{} ({route:?}): call resources must be released before session cleanup",
            case.name
        );
    };
    if case.cancel_call.is_some() || case.cancel_at.is_some() {
        let mut options = vm.execution_options();
        options.cancellation = cancellation.clone();
        let mut session = vm.begin_execution(&selected, options).unwrap();
        run(session.runtime_mut());
    } else {
        run(&mut vm);
    }
    if let Some(reload) = &case.published_reload {
        let candidate = reload.candidate;
        let program = candidate_program.expect("published candidate must compile");
        // Queue the exact old body before publication; no synchronous activation
        // remains while the host publishes. Its ordinary calls still select current.
        let old_execution = vm
            .start(&selected, ProgramEntry::Main, &[], vm.execution_options())
            .unwrap();
        let stale = vm
            .stage_reload_verified_program(&loaded, None, program.program().bytecode().clone())
            .unwrap();
        vm.program(&loaded)
            .map_err(ReloadValidationError::Runtime)
            .and_then(|base| base.prepare_replacement(program.program(), &Default::default()))
            .and_then(|candidate| vm.replace_program(&loaded, &candidate, Default::default()))
            .unwrap();
        let current = loaded.clone();
        let current_selected = vm.select_program(&current).unwrap();
        let current_native = matches!(route, Route::Jit | Route::ArtifactJit).then(|| {
            prepare_native(
                &mut vm,
                &mut compiler,
                program,
                &current,
                ProgramEntry::Main,
                &mut backend,
            )
        });
        assert_ne!(current_selected.key(), selected.key());
        assert!(vm.execution_root().is_none());
        let DriveResult::Complete(result) = vm
            .drive(&old_execution, NonZeroUsize::new(1024).unwrap())
            .unwrap()
        else {
            panic!(
                "{} ({route:?}): queued scalar entry must complete",
                case.name
            );
        };
        let result = result.unwrap();
        let expected =
            (reload.selected_result)(&mut ConversionContext::new(&mut vm, &selected).unwrap());
        assert!(
            script_equal(&mut vm, &result, &expected).unwrap(),
            "{} ({route:?}): old body must call the current dependency",
            case.name
        );
        drop(expected);
        drop(result);
        if let Some(entries) = &old_native {
            for (entry, prepared) in entries {
                match prepared {
                    PreparedNativeEntry::Native(_) => {
                        // An exact native product cannot be reused for the current
                        // entry, even when the host explicitly requests fallback.
                        for fallback_reason in [None, Some("interpret current entry")] {
                            let error = vm
                                .execute_prepared_module(
                                    &selected,
                                    *entry,
                                    PreparedNativeExecution {
                                        prepared,
                                        fallback_reason,
                                    },
                                    vm.execution_options(),
                                )
                                .unwrap_err();
                            assert!(
                                matches!(
                                    &error,
                                    error
                                        if error.kind() == RuntimeErrorKind::ModuleValidation
                                ),
                                "{} ({route:?}): {error:?}",
                                case.name
                            );
                        }
                    }
                    PreparedNativeEntry::Unsupported { .. } => {
                        let outcome = vm.execute_prepared_module(
                            &selected,
                            ProgramEntry::Main,
                            prepared,
                            vm.execution_options(),
                        );
                        assert_eq!(
                            outcome.as_ref().unwrap().jit.as_ref().unwrap().status,
                            JitExecutionStatus::InterpreterFallback
                        );
                        assert_outcome(&mut vm, &current_selected, candidate, route, 0, outcome);
                    }
                }
            }
        }
        let outcome = vm.execute_module(&selected, ProgramEntry::Main, vm.execution_options());
        assert_outcome(&mut vm, &current_selected, candidate, route, 0, outcome);
        let outcome = execute_route(&mut vm, &current, candidate, route, current_native.as_ref());
        assert_outcome(&mut vm, &current_selected, candidate, route, 0, outcome);
        let before = vm.modules().loaded_count();
        let stale_members = stale.module().members().count();
        assert!(matches!(
            vm.publish_staged_reload(stale),
            Err(ReloadValidationError::ModuleNotActive { .. })
        ));
        assert_eq!(
            vm.select_program(&current).unwrap().key(),
            current_selected.key()
        );
        assert_eq!(vm.modules().loaded_count(), before - stale_members);
        let outcome = execute_route(&mut vm, &current, candidate, route, current_native.as_ref());
        assert_outcome(&mut vm, &current_selected, candidate, route, 1, outcome);
    }
    assert_eq!(
        vm.resources().counters().current_call_depth,
        0,
        "{} ({route:?}): call resources must be released after success or failure",
        case.name
    );
    if let Some((_, expected)) = case.vector {
        vm.collect_garbage().unwrap();
        assert_eq!(
            vm.gc_stats().root_registrations,
            retained_roots,
            "calls release their roots and preserve the explicit observer root"
        );
        let owner = vm.select_program(&loaded).unwrap();
        let mut cx = ConversionContext::new(&mut vm, &owner).unwrap();
        let array = cx.import(retained.as_ref().unwrap()).unwrap();
        assert_eq!(cx.array_len(&array).unwrap(), expected.len());
        let actual = (0..expected.len())
            .map(|index| {
                cx.array_element(&array, index)
                    .unwrap()
                    .unwrap()
                    .as_i32()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actual, expected,
            "{} ({route:?}): post-execution heap state",
            case.name
        );
    }
    let host = host.lock().unwrap();
    let mut expected_calls = case
        .calls
        .iter()
        .map(|message| HostCall {
            symbol: "host.log",
            args: vec![(*message).into()],
        })
        .collect::<Vec<_>>();
    if case.vector.is_some() {
        expected_calls.insert(
            0,
            HostCall {
                symbol: "observe.vector",
                args: vec![],
            },
        );
    }
    let expected_mutations = case
        .committed
        .iter()
        .enumerate()
        .map(|(previous_len, message)| Mutation {
            target: "host.log",
            previous_len,
            appended: (*message).into(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        host.calls, expected_calls,
        "{} ({route:?}): host calls",
        case.name
    );
    assert_eq!(
        host.mutations, expected_mutations,
        "{} ({route:?}): mutation records",
        case.name
    );
    assert_eq!(
        host.log, case.committed,
        "{} ({route:?}): committed state",
        case.name
    );
}

mod evaluation;
mod preparation;
mod types_and_dispatch;
