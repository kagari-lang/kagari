//! One observable suite for source, artifacts and the existing JIT/fallback.
//! No bytecode layouts or arena IDs appear in the fixture expectations.
use crate::{
    context::{ExecutionContext, JitPolicy},
    engine::source::PreparationOptions,
    error::EmbeddingError,
    program::PreparedProgram,
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
use kagari_runtime::{
    Runtime as CoreRuntime, RuntimeConfig,
    error::{RuntimeError, RuntimeErrorKind},
    frame::ExecutionFrame,
    gc::GcHeap,
    host::{HostCallContext, HostError},
    module::{Program, entry::ProgramEntry},
    native::{
        builder::ModuleBuilder, conversion::context::ConversionContext, module::NativeModule,
        types::Type,
    },
    reload::ReloadValidationError,
    resource::RuntimeLimits,
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
    value_semantics::script_equal,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::{collections::vector::VecValues, declarations::StandardDeclarations};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, host::HostCallPolicy, logging, module::ModuleDecl},
};
use kagari_vm::{
    error::VmError,
    vm::{
        ExecutionReport, JitExecutionStatus,
        native::{PreparedNativeEntry, PreparedNativeExecution},
        owned::DriveResult,
    },
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
    Value(fn(&GcHeap) -> Value),
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
    selected_result: fn(&GcHeap) -> Value,
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
    entry: impl for<'call> Fn(&HostCallContext<'call>, &[Value]) -> Result<Value, HostError>
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
    builder.bind(log, entry).unwrap();
    builder.finish().unwrap()
}

fn observe_vector() -> DefinitionMetadata<ModuleDecl> {
    let context = DefinitionContext::new().unwrap();
    let vector = StandardDeclarations::default().vec(Type::i32()).unwrap();
    let vector = DefinitionMetadata::checked(
        vector.definitions().unwrap().clone(),
        vector.abi().clone(),
        &Default::default(),
    )
    .unwrap()
    .import_into(&context, &Default::default())
    .unwrap()
    .into_records();
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
    let encoded = PreparedProgram::from_artifact(
        encoded,
        &ArtifactCompatibility::default(),
        &Default::default(),
    )
    .unwrap();
    Some(RoutePrograms { source, encoded })
}

struct RoutePrograms {
    source: PreparedProgram,
    encoded: PreparedProgram,
}

impl RoutePrograms {
    fn get(&self, route: Route) -> &PreparedProgram {
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
    runtime: &CoreRuntime,
    case: &Case<'_>,
    route: Route,
    attempt: usize,
    outcome: Result<ExecutionReport, VmError>,
) {
    match (&case.expected, outcome.as_ref().map_err(VmError::cause)) {
        (Expected::Value(expected), Ok(report)) => {
            let actual = report
                .return_value
                .value(runtime.gc())
                .expect("retained execution result");
            assert!(
                script_equal(runtime.gc(), &actual, &expected(runtime.gc())).unwrap(),
                "{} ({route:?}, attempt {attempt})",
                case.name
            );
        }
        (Expected::IndexTrap, Err(VmError::InvalidIndex(_))) => {}
        (Expected::IndexTrap, Err(VmError::RuntimeError(error)))
            if error.kind() == RuntimeErrorKind::IndexOutOfBounds => {}
        (Expected::HostFailure, Err(VmError::RuntimeError(error)))
            if error.kind() == RuntimeErrorKind::HostCallFailure => {}
        (Expected::ScriptTrap(message), Err(VmError::RuntimeError(error)))
            if error.kind() == RuntimeErrorKind::ScriptTrap && error.message() == *message => {}
        (Expected::Cancelled, Err(VmError::RuntimeError(error)))
            if error.kind() == RuntimeErrorKind::Cancelled => {}
        (expected, actual) => panic!(
            "{} ({route:?}, attempt {attempt}): expected {expected:?}, got {actual:?}",
            case.name
        ),
    }
}

fn execute_route(
    runtime: &Runtime,
    program: &PreparedProgram,
    loaded: &Program,
    case: &Case<'_>,
    route: Route,
    backend: &mut CraneliftBackend,
) -> Result<ExecutionReport, VmError> {
    let selected = runtime.runtime().select_program(loaded)?;
    match route {
        Route::Jit | Route::ArtifactJit => {
            let prepared = runtime
                .prepare_native(
                    program,
                    loaded,
                    ProgramEntry::Main,
                    backend,
                    &Default::default(),
                )
                .unwrap();
            if case.require_native {
                assert!(
                    matches!(prepared, PreparedNativeEntry::Native(_)),
                    "{} ({route:?}) must enter native code: {prepared:?}",
                    case.name
                );
            }
            // Retain the VM's exact error variants in this compiler/SDK contract
            // matrix. Native preparation uses the public SDK's trusted handoff.
            let outcome = runtime
                .vm
                .execute_prepared(&selected, ProgramEntry::Main, &prepared);
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
        _ => runtime.vm.execute(&selected, ProgramEntry::Main),
    }
}

fn run(
    case: &Case<'_>,
    route: Route,
    module: &PreparedProgram,
    candidate_program: Option<&PreparedProgram>,
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
        let [Value::Str(id)] = args else {
            return Err(HostError::new("log requires one string"));
        };
        let message = cx.runtime().gc().string(*id).unwrap().to_owned();
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
        Ok(Value::Unit)
    })
    .install(&mut runtime)
    .unwrap();
    let observed_vector = Arc::new(Mutex::new(None));
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
            .bind(vector, move |_: &HostCallContext<'_>, _: &[Value]| {
                capture_host.lock().unwrap().calls.push(HostCall {
                    symbol: "observe.vector",
                    args: vec![],
                });
                (*capture_vector.lock().unwrap())
                    .ok_or_else(|| HostError::new("Vec fixture is not initialized"))
            })
            .unwrap();
        builder.finish().unwrap().install(&mut runtime).unwrap();
    }
    let loaded = runtime
        .load_verified_program(case.name, module.bytecode().clone())
        .unwrap_or_else(|error| panic!("{} ({route:?}): {error}", case.name));
    let selected = runtime.select_program(&loaded).unwrap();
    let retained = case.vector.map(|(initial, _)| {
        let mut conversion = ConversionContext::new(&runtime, &selected).unwrap();
        let rooted = conversion.encode(VecValues(initial.to_vec())).unwrap();
        *observed_vector.lock().unwrap() = Some(rooted.value(runtime.gc()).unwrap());
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
    let vm = Runtime::new(runtime, context);
    let session = (case.cancel_call.is_some() || case.cancel_at.is_some()).then(|| {
        let mut options = vm.runtime().execution_options();
        options.cancellation = cancellation.clone();
        vm.runtime().begin_execution(&selected, options).unwrap()
    });
    if case.cancel_at.is_some() {
        vm.runtime().attach_execution_observer().unwrap();
    }
    let mut backend = CraneliftBackend::for_host().unwrap();
    for attempt in 0..case.repeat {
        let outcome = execute_route(&vm, module, &loaded, case, route, &mut backend);
        assert_outcome(vm.runtime(), case, route, attempt, outcome);
    }
    if let Some(reload) = &case.published_reload {
        let candidate = reload.candidate;
        let program = candidate_program.expect("published candidate must compile");
        let old_native = matches!(route, Route::Jit | Route::ArtifactJit).then(|| {
            [
                ProgramEntry::Main,
                ProgramEntry::Export(reload.native_entry),
            ]
            .map(|entry| {
                let prepared = vm
                    .prepare_native(module, &loaded, entry, &mut backend, &Default::default())
                    .unwrap();
                if matches!(entry, ProgramEntry::Export(_)) {
                    assert!(
                        matches!(prepared, PreparedNativeEntry::Native(_)),
                        "{} ({route:?}): dependency must be native: {prepared:?}",
                        case.name
                    );
                }
                (entry, prepared)
            })
        });
        // Queue the exact old body before publication; no synchronous activation
        // remains while the host publishes. Its ordinary calls still select current.
        let old_execution = vm
            .vm
            .start(
                &selected,
                ProgramEntry::Main,
                &[],
                vm.runtime().execution_options(),
            )
            .unwrap();
        let stale = vm
            .runtime()
            .stage_reload_verified_program(&loaded, None, program.bytecode().clone())
            .unwrap();
        vm.reload_program(&loaded, program, Default::default())
            .unwrap();
        let current = loaded.clone();
        let current_selected = vm.runtime().select_program(&current).unwrap();
        assert_ne!(current_selected.key(), selected.key());
        assert!(vm.runtime().execution_root().is_none());
        let DriveResult::Complete(result) = vm
            .vm
            .drive(&old_execution, NonZeroUsize::new(1024).unwrap())
            .unwrap()
        else {
            panic!(
                "{} ({route:?}): queued scalar entry must complete",
                case.name
            );
        };
        let result = result.unwrap();
        assert!(
            script_equal(
                vm.runtime().gc(),
                &result.value(vm.runtime().gc()).unwrap(),
                &(reload.selected_result)(vm.runtime().gc()),
            )
            .unwrap(),
            "{} ({route:?}): old body must call the current dependency",
            case.name
        );
        drop(result);
        if let Some(entries) = &old_native {
            for (entry, prepared) in entries {
                match prepared {
                    PreparedNativeEntry::Native(_) => {
                        // An exact native product cannot be reused for the current
                        // entry, even when the host explicitly requests fallback.
                        for fallback_reason in [None, Some("interpret current entry")] {
                            let error = vm
                                .vm
                                .execute_prepared(
                                    &selected,
                                    *entry,
                                    PreparedNativeExecution {
                                        prepared,
                                        fallback_reason,
                                    },
                                )
                                .unwrap_err();
                            assert!(
                                matches!(
                                    error.cause(),
                                    VmError::RuntimeError(error)
                                        if error.kind() == RuntimeErrorKind::ModuleValidation
                                ),
                                "{} ({route:?}): {error:?}",
                                case.name
                            );
                        }
                    }
                    PreparedNativeEntry::Unsupported { .. } => {
                        let outcome =
                            vm.vm
                                .execute_prepared(&selected, ProgramEntry::Main, prepared);
                        assert_eq!(
                            outcome.as_ref().unwrap().jit.as_ref().unwrap().status,
                            JitExecutionStatus::InterpreterFallback
                        );
                        assert_outcome(vm.runtime(), candidate, route, 0, outcome);
                    }
                }
            }
        }
        assert_outcome(
            vm.runtime(),
            candidate,
            route,
            0,
            vm.vm.execute(&selected, ProgramEntry::Main),
        );
        assert_outcome(
            vm.runtime(),
            candidate,
            route,
            0,
            execute_route(&vm, program, &current, candidate, route, &mut backend),
        );
        let before = vm.runtime().modules().loaded_count();
        let stale_members = stale.module().members().count();
        assert!(matches!(
            vm.runtime().publish_staged_reload(stale),
            Err(ReloadValidationError::ModuleNotActive { .. })
        ));
        assert_eq!(
            vm.runtime().select_program(&current).unwrap().key(),
            current_selected.key()
        );
        assert_eq!(
            vm.runtime().modules().loaded_count(),
            before - stale_members
        );
        assert_outcome(
            vm.runtime(),
            candidate,
            route,
            1,
            execute_route(&vm, program, &current, candidate, route, &mut backend),
        );
    }
    assert_eq!(
        vm.runtime().resources().counters().current_call_depth,
        0,
        "{} ({route:?}): call resources must be released after success or failure",
        case.name
    );
    drop(session);
    if let Some((_, expected)) = case.vector {
        assert_eq!(
            vm.runtime().gc().active_roots(),
            1,
            "only the explicit observer root remains"
        );
        vm.runtime().collect_garbage().unwrap();
        let mut conversion = ConversionContext::new(vm.runtime(), &selected).unwrap();
        assert_eq!(
            conversion
                .decode::<VecValues<i32>>(retained.as_ref().unwrap())
                .unwrap()
                .0,
            expected,
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
