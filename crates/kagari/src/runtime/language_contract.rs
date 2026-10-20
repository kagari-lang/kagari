//! One observable suite for source, artifacts and the existing JIT/fallback.
//! No bytecode layouts or arena IDs appear in the fixture expectations.
use crate::{
    context::{ExecutionContext, JitPolicy},
    program::PreparedProgram,
    runtime::KagariRuntime,
};
use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, ArtifactCompatibility, KbcArtifact},
    native_input::PortableMir,
};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
        metadata::DefinitionMetadata,
    },
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::program::ProgramCheckError;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::{RuntimeError, RuntimeErrorKind},
    frame::ExecutionFrame,
    gc::GcHeap,
    host::{HostCallContext, HostError},
    module::{LoadedModule, entry::ProgramEntry},
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
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, host::HostCallPolicy, logging, module::ModuleDecl},
};
use kagari_vm::{
    error::VmError,
    vm::{ExecutionReport, JitExecutionStatus, native::PreparedNativeEntry},
};
use std::{
    cell::Cell,
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
        runtime: &Runtime,
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

struct Case<'a> {
    name: &'static str,
    source: &'a str,
    modules: &'a [(&'a str, &'a str)],
    published_reload: Option<&'a Case<'a>>,
    expected: Expected,
    calls: &'static [&'static str],
    committed: &'static [&'static str],
    reject_call: Option<usize>,
    cancel_call: Option<usize>,
    repeat: usize,
    require_native: bool,
    cancel_at: Option<usize>,
    reflection: bool,
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
            reflection: false,
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

    fn reflection(mut self) -> Self {
        self.reflection = true;
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

fn compile(case: &Case<'_>) -> Option<DefinitionMetadata<KbcArtifact>> {
    let observe = observe_vector();
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
    let mut analysis = preparation::analysis();
    if case.vector.is_some() {
        let mut providers = kagari_stdlib::modules()
            .unwrap()
            .iter()
            .map(|module| Arc::new(module.to_declaration()))
            .collect::<Vec<_>>();
        providers.push(Arc::new(observe));
        analysis.set_native_modules(providers);
    }
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root.unwrap(), &Default::default());
    if let Expected::Diagnostic(code) = case.expected {
        let Err(ProgramCheckError::Diagnostics(diagnostics)) = checked else {
            let outcome = checked.map(|_| "checked successfully");
            panic!("{}: expected diagnostic {code}, got {outcome:?}", case.name);
        };
        assert!(
            diagnostics.iter().any(|d| d.diagnostic.kind.code() == code),
            "{}: {diagnostics:?}",
            case.name
        );
        return None;
    }
    let checked = checked.unwrap_or_else(|error| panic!("{}: {error:?}", case.name));
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let compiled = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let artifact = KbcArtifact::from_program(
        compiled,
        ArtifactBuildOptions {
            portable_mir: Some(PortableMir {
                bytes: kagari_mir::codec::encode_program(&mir, &Default::default()).unwrap(),
            }),
            ..Default::default()
        },
        &context,
        &Default::default(),
    )
    .unwrap();
    Some(DefinitionMetadata::checked(context.snapshot(), artifact, &Default::default()).unwrap())
}

struct RoutePrograms {
    source: PreparedProgram,
    encoded: PreparedProgram,
}

impl RoutePrograms {
    fn new(artifact: DefinitionMetadata<KbcArtifact>) -> Self {
        let encoded =
            KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
                .unwrap();
        let prepare = |artifact| {
            PreparedProgram::from_artifact(
                artifact,
                &ArtifactCompatibility::default(),
                &Default::default(),
            )
            .unwrap()
        };
        Self {
            source: prepare(artifact),
            encoded: prepare(encoded),
        }
    }

    fn get(&self, route: Route) -> &PreparedProgram {
        match route {
            Route::Source | Route::Jit => &self.source,
            Route::Artifact | Route::ArtifactJit => &self.encoded,
        }
    }
}

fn run_routes(case: &Case<'_>) {
    let Some(artifact) = compile(case) else {
        return;
    };
    // Interpreter and JIT routes consume the same checked input. Validate direct
    // and serialized products separately; every route still gets a fresh runtime.
    let programs = RoutePrograms::new(artifact);
    let candidate = case
        .published_reload
        .map(|case| RoutePrograms::new(compile(case).expect("published candidate must compile")));
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
    runtime: &Runtime,
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
    runtime: &KagariRuntime,
    program: &PreparedProgram,
    loaded: &LoadedModule,
    case: &Case<'_>,
    route: Route,
    backend: &mut CraneliftBackend,
) -> Result<ExecutionReport, VmError> {
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
                .execute_prepared(loaded, ProgramEntry::Main, &prepared);
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
        _ => runtime.vm.execute(loaded, ProgramEntry::Main),
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
    let retained = case.vector.map(|(initial, _)| {
        let mut conversion = ConversionContext::new(&runtime, &loaded).unwrap();
        let rooted = conversion.encode(initial.to_vec()).unwrap();
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
    let vm = KagariRuntime::new(runtime, context);
    let session = (case.cancel_call.is_some() || case.cancel_at.is_some()).then(|| {
        let mut options = vm.runtime().execution_options();
        options.cancellation = cancellation.clone();
        vm.runtime().begin_execution(&loaded, options).unwrap()
    });
    if case.cancel_at.is_some() {
        vm.runtime().attach_execution_observer().unwrap();
    }
    let mut backend = CraneliftBackend::for_host().unwrap();
    for attempt in 0..case.repeat {
        let outcome = execute_route(&vm, module, &loaded, case, route, &mut backend);
        assert_outcome(vm.runtime(), case, route, attempt, outcome);
    }
    if let Some(candidate) = case.published_reload {
        let program = candidate_program.expect("published candidate must compile");
        let outer = vm
            .runtime()
            .begin_execution(&loaded, vm.runtime().execution_options())
            .unwrap();
        let stale = vm
            .runtime()
            .stage_reload_verified_program(&loaded, case.name, program.bytecode().clone())
            .unwrap();
        let current = vm
            .reload_program(&loaded, program, Default::default())
            .unwrap();
        assert_eq!(
            vm.runtime().modules().latest(case.name).unwrap().key(),
            current.key()
        );
        assert_ne!(current.key(), loaded.key());
        assert_eq!(vm.runtime().execution_root().unwrap().key(), loaded.key());
        assert_outcome(
            vm.runtime(),
            case,
            route,
            case.repeat,
            execute_route(&vm, module, &loaded, case, route, &mut backend),
        );
        drop(outer);
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
            vm.runtime().modules().latest(case.name).unwrap().key(),
            current.key()
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
        let Value::GcHandle(array) = retained.as_ref().unwrap().value(vm.runtime().gc()).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(
            vm.runtime().gc().sequence_snapshot(array),
            Some(expected.iter().copied().map(Value::I32).collect()),
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
