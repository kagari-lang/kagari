//! One observable suite for source, artifacts and the existing JIT/fallback.
//! No bytecode layouts or arena IDs appear in the fixture expectations.
use std::{
    cell::Cell,
    sync::{Arc, Mutex},
};

use kagari_hir::LanguageFeatureProfile;
use kagari_ir::{
    bytecode::{
        ArtifactBuildOptions, ArtifactCompatibility, KbcArtifact, lower_program_to_bytecode,
    },
    program::lower_program_to_ir,
};
use kagari_jit_cranelift::CraneliftBackend;
use kagari_runtime::{
    CapabilitySet, HostExposurePolicy, LanguageProfile, Runtime, RuntimeConfig, SecurityContext,
    host::{HostError, HostFunction},
    value::Value,
};

use crate::{Vm, VmError};

struct RecordingBackend {
    inner: CraneliftBackend,
    invocations: Cell<usize>,
}
impl kagari_runtime::CodegenBackend for RecordingBackend {
    fn backend_id(&self) -> kagari_runtime::BackendId {
        self.inner.backend_id()
    }
    fn target(&self) -> kagari_runtime::BackendTarget {
        self.inner.target()
    }
    fn compile_function(
        &mut self,
        input: kagari_runtime::BackendFunctionInput<'_>,
    ) -> Result<kagari_runtime::ExecutableFunctionArtifact, kagari_runtime::BackendCompileError>
    {
        self.inner.compile_function(input)
    }
    fn invoke_function(
        &self,
        artifact: &kagari_runtime::ExecutableFunctionArtifact,
        runtime: &Runtime,
    ) -> Result<Value, kagari_runtime::BackendInvocationError> {
        self.invocations.set(self.invocations.get() + 1);
        self.inner.invoke_function(artifact, runtime)
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
    Value(Value),
    Diagnostic(&'static str),
    ImportCycle,
    IndexTrap,
    HostFailure,
    ScriptTrap(&'static str),
    BuiltinTrap(String),
    ResourceLimit,
    Cancelled,
    CapabilityDenied,
}

#[derive(Debug, PartialEq)]
struct HostCall {
    symbol: &'static str,
    args: Vec<Value>,
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
    rejected_reload: Option<&'a Case<'a>>,
    published_reload: Option<&'a Case<'a>>,
    expected: Expected,
    calls: &'static [&'static str],
    committed: &'static [&'static str],
    reject_call: Option<usize>,
    cancel_call: Option<usize>,
    repeat: usize,
    require_native: bool,
    max_steps: Option<u64>,
    reflection: bool,
    iterating: bool,
    array: Option<(&'static [i32], &'static [i32])>,
}

impl<'a> Case<'a> {
    fn new(name: &'static str, source: &'a str, expected: Expected) -> Self {
        Self {
            name,
            source,
            modules: &[],
            rejected_reload: None,
            published_reload: None,
            expected,
            calls: &[],
            committed: &[],
            reject_call: None,
            cancel_call: None,
            repeat: 1,
            require_native: false,
            max_steps: None,
            reflection: false,
            iterating: false,
            array: None,
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

    fn array(mut self, initial: &'static [i32], expected: &'static [i32]) -> Self {
        self.array = Some((initial, expected));
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

fn compile(case: &Case<'_>, route: Route) -> Option<kagari_ir::bytecode::BytecodeProgram> {
    let profile = LanguageFeatureProfile {
        allow_host_calls: true,
        allow_reflection: case.reflection,
        allow_reflection_write: case.reflection,
        ..Default::default()
    };
    let observe = kagari_common::host_interface::HostFunctionDeclaration::new(
        "observe.array",
        vec![],
        kagari_common::host_interface::HostValueType::Array(Box::new(
            kagari_common::host_interface::HostValueType::I32,
        )),
    );
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::{SourceDatabase, SourceLayer},
    };
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
    let mut analysis = kagari_hir::analysis::AnalysisDatabase::default();
    if case.array.is_some() {
        analysis.set_host_declarations(
            kagari_hir::host::HostDeclarations::new(kagari_common::host_interface::HostInterface {
                paths: vec![],
                types: vec![],
                functions: vec![observe.clone()],
            })
            .unwrap(),
        );
    }
    let snapshot = analysis
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root.unwrap(), &Default::default());
    if let Expected::Diagnostic(code) = case.expected {
        let Err(kagari_hir::program::ProgramCheckError::Diagnostics(diagnostics)) = checked else {
            panic!(
                "{} ({route:?}): expected diagnostic {code}, got {checked:?}",
                case.name
            );
        };
        assert!(
            diagnostics.iter().any(|d| d.diagnostic.kind.code() == code),
            "{} ({route:?}): {diagnostics:?}",
            case.name
        );
        return None;
    }
    if matches!(case.expected, Expected::ImportCycle) {
        assert!(
            matches!(
                checked,
                Err(kagari_hir::program::ProgramCheckError::Graph(
                    kagari_hir::imports::ModuleOrderError::Cycle(_)
                ))
            ),
            "{} ({route:?}): {checked:?}",
            case.name
        );
        return None;
    }
    let checked = checked.unwrap_or_else(|error| panic!("{} ({route:?}): {error:?}", case.name));
    let compiled =
        lower_program_to_bytecode(&lower_program_to_ir(&checked, &Default::default()).unwrap())
            .unwrap();
    let module = match route {
        Route::Source | Route::Jit => compiled,
        Route::Artifact | Route::ArtifactJit => {
            let bytes = KbcArtifact::from_program(compiled, ArtifactBuildOptions::default())
                .unwrap()
                .to_bytes()
                .unwrap();
            let decoded = KbcArtifact::from_bytes(&bytes).unwrap();
            decoded
                .validate_for_loader(&ArtifactCompatibility::default())
                .unwrap();
            decoded.program
        }
    };
    Some(module)
}

fn assert_outcome(
    case: &Case<'_>,
    route: Route,
    attempt: usize,
    outcome: Result<crate::ExecutionReport, VmError>,
) {
    match (&case.expected, outcome) {
        (Expected::Value(expected), Ok(report)) => assert_eq!(
            &report.return_value, expected,
            "{} ({route:?}, attempt {attempt})",
            case.name
        ),
        (Expected::IndexTrap, Err(VmError::InvalidIndex(_))) => {}
        (Expected::HostFailure, Err(VmError::RuntimeError(error)))
            if error.kind() == kagari_runtime::RuntimeErrorKind::HostCallFailure => {}
        (Expected::ScriptTrap(message), Err(VmError::RuntimeError(error)))
            if error.kind() == kagari_runtime::RuntimeErrorKind::ScriptTrap
                && error.message() == *message => {}
        (Expected::BuiltinTrap(message), Err(VmError::BuiltinError(error)))
            if error.kind() == kagari_runtime::RuntimeErrorKind::ScriptTrap
                && error.message() == message => {}
        (Expected::ResourceLimit, Err(VmError::RuntimeError(error)))
            if error.kind() == kagari_runtime::RuntimeErrorKind::ResourceLimitExceeded => {}
        (Expected::Cancelled, Err(VmError::RuntimeError(error)))
            if error.kind() == kagari_runtime::RuntimeErrorKind::Cancelled => {}
        (Expected::CapabilityDenied, Err(VmError::RuntimeError(error)))
            if error.kind() == kagari_runtime::RuntimeErrorKind::CapabilityDenied => {}
        (expected, actual) => panic!(
            "{} ({route:?}, attempt {attempt}): expected {expected:?}, got {actual:?}",
            case.name
        ),
    }
}

fn execute_route(
    vm: &mut Vm,
    loaded: &kagari_runtime::LoadedModule,
    route: Route,
    backend: &mut RecordingBackend,
) -> Result<crate::ExecutionReport, VmError> {
    match route {
        Route::Jit | Route::ArtifactJit => vm.execute_with_backend(loaded, "main", backend),
        _ => vm.execute(loaded, "main"),
    }
}

fn run(case: &Case<'_>, route: Route) {
    let Some(module) = compile(case, route) else {
        return;
    };
    let observe = kagari_common::host_interface::HostFunctionDeclaration::new(
        "observe.array",
        vec![],
        kagari_common::host_interface::HostValueType::Array(Box::new(
            kagari_common::host_interface::HostValueType::I32,
        )),
    );
    let mut runtime = Runtime::new(RuntimeConfig {
        resources: kagari_runtime::ResourcePolicy {
            max_instruction_steps: case.max_steps,
            ..Default::default()
        },
        security: SecurityContext {
            profile: LanguageProfile {
                allow_jit: true,
                allow_host_calls: true,
                allow_reflection: case.reflection,
                allow_reflection_write: case.reflection,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                jit: true,
                host_calls: true,
                reflection_metadata: case.reflection,
                reflection_read: case.reflection,
                reflection_write: case.reflection,
                ..Default::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allowed_host_functions: vec!["host.log".into(), "observe.array".into()],
            ..Default::default()
        },
        ..Default::default()
    });
    let host = Arc::new(Mutex::new(RecordingHost::default()));
    let capture = host.clone();
    let reject_call = case.reject_call;
    let cancel_call = case.cancel_call;
    let cancellation = kagari_common::cancellation::CancellationToken::default();
    let cancel = cancellation.clone();
    runtime
        .register_host_function(HostFunction::new(
            kagari_common::host_interface::standard_log(),
            move |_, args| {
                let mut state = capture.lock().unwrap();
                state.calls.push(HostCall {
                    symbol: "host.log",
                    args: args.to_vec(),
                });
                if reject_call == Some(state.calls.len()) {
                    return Err(HostError::new("test host rejected append"));
                }
                let [Value::Str(message)] = args else {
                    return Err(HostError::new("log requires one string"));
                };
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
            },
        ))
        .unwrap();
    let retained = case.array.map(|(initial, _)| {
        let array = runtime
            .alloc_array(initial.iter().copied().map(Value::I32).collect())
            .unwrap();
        let rooted = std::rc::Rc::new(runtime.root_value(Value::Array(array)).unwrap());
        let capture_root = rooted.clone();
        let capture_host = host.clone();
        runtime
            .register_host_function(HostFunction::new(observe, move |_, _| {
                capture_host.lock().unwrap().calls.push(HostCall {
                    symbol: "observe.array",
                    args: vec![],
                });
                Ok(capture_root.value())
            }))
            .unwrap();
        rooted
    });
    let loaded = runtime.load_program(case.name, module).unwrap();
    let iteration = case.iterating.then(|| {
        runtime
            .gc()
            .begin_collection_iteration(
                &retained
                    .as_ref()
                    .expect("iteration fixture needs an array")
                    .value(),
            )
            .unwrap()
    });
    let session = case.cancel_call.map(|_| {
        let mut options = runtime.execution_options();
        options.cancellation = cancellation;
        runtime.begin_execution(&loaded, options).unwrap()
    });
    let mut vm = Vm::new(runtime);
    let mut backend = RecordingBackend {
        inner: CraneliftBackend::for_host().unwrap(),
        invocations: Cell::new(0),
    };
    for attempt in 0..case.repeat {
        let outcome = execute_route(&mut vm, &loaded, route, &mut backend);
        assert_outcome(case, route, attempt, outcome);
    }
    if matches!(route, Route::Jit | Route::ArtifactJit) && case.require_native {
        assert_eq!(
            backend.invocations.get(),
            case.repeat,
            "{} must actually invoke native code",
            case.name
        );
    }
    if let Some(candidate) = case.rejected_reload {
        let before = vm.runtime().resources().counters().loaded_modules;
        let program = compile(candidate, route).expect("reload candidate must compile");
        let error = vm.reload_program(&loaded, case.name, program).unwrap_err();
        let crate::ReloadError::Initialization(error) = error else {
            panic!(
                "{} candidate {} ({route:?}): {error:?}",
                case.name, candidate.name
            );
        };
        assert_outcome(candidate, route, 0, Err(error));
        assert_eq!(
            vm.runtime().modules().latest(case.name).unwrap().key(),
            loaded.key()
        );
        assert_eq!(vm.runtime().resources().counters().loaded_modules, before);
        let outcome = execute_route(&mut vm, &loaded, route, &mut backend);
        assert_outcome(case, route, case.repeat, outcome);
    }
    if let Some(candidate) = case.published_reload {
        let program = compile(candidate, route).expect("published candidate must compile");
        let outer = vm
            .runtime()
            .begin_execution(&loaded, vm.runtime().execution_options())
            .unwrap();
        let stale = vm
            .runtime_mut()
            .stage_reload_program(&loaded, case.name, program.clone())
            .unwrap();
        {
            let isolated = vm.runtime().begin_candidate_initialization(&stale).unwrap();
            vm.execute_module(stale.module()).unwrap();
            drop(isolated);
        }
        let current = vm.reload_program(&loaded, case.name, program).unwrap();
        assert_eq!(
            vm.runtime().modules().latest(case.name).unwrap().key(),
            current.key()
        );
        assert_ne!(current.key(), loaded.key());
        assert_eq!(vm.runtime().execution_root().unwrap().key(), loaded.key());
        assert_outcome(
            case,
            route,
            case.repeat,
            execute_route(&mut vm, &loaded, route, &mut backend),
        );
        drop(outer);
        assert_outcome(
            candidate,
            route,
            0,
            execute_route(&mut vm, &current, route, &mut backend),
        );
        let before = vm.runtime().resources().counters().loaded_modules;
        let stale_members = stale.module().members().count();
        assert!(matches!(
            vm.runtime_mut().publish_staged_reload(stale),
            Err(kagari_runtime::ReloadValidationError::ModuleNotActive { .. })
        ));
        assert_eq!(
            vm.runtime().modules().latest(case.name).unwrap().key(),
            current.key()
        );
        assert_eq!(
            vm.runtime().resources().counters().loaded_modules,
            before - stale_members
        );
        assert_outcome(
            candidate,
            route,
            1,
            execute_route(&mut vm, &current, route, &mut backend),
        );
    }
    assert_eq!(
        vm.runtime().resources().counters().current_call_depth,
        0,
        "{} ({route:?}): call resources must be released after success or failure",
        case.name
    );
    drop(session);
    drop(iteration);
    if let Some((_, expected)) = case.array {
        assert_eq!(
            vm.runtime().gc().active_roots(),
            1,
            "only the explicit observer root remains"
        );
        vm.runtime().collect_garbage().unwrap();
        let Value::Array(array) = retained.as_ref().unwrap().value() else {
            unreachable!()
        };
        assert_eq!(
            vm.runtime().gc().array_snapshot(array),
            Some(expected.iter().copied().map(Value::I32).collect()),
            "{} ({route:?}): post-execution heap state",
            case.name
        );
    }
    if case.iterating {
        let Value::Array(array) = retained.as_ref().unwrap().value() else {
            unreachable!()
        };
        vm.runtime().gc().array_push(array, Value::I32(99)).unwrap();
        assert_eq!(
            vm.runtime().gc().array_pop(array).unwrap(),
            Some(Value::I32(99)),
            "released guard must permit structural mutation"
        );
    }
    let host = host.lock().unwrap();
    let mut expected_calls = case
        .calls
        .iter()
        .map(|message| HostCall {
            symbol: "host.log",
            args: vec![Value::Str((*message).into())],
        })
        .collect::<Vec<_>>();
    if case.array.is_some() {
        expected_calls.insert(
            0,
            HostCall {
                symbol: "observe.array",
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

#[test]
fn language_contract_routes_preserve_values_diagnostics_and_effects() {
    let generic_aggregates = Case::new(
        "generic-aggregate-layout-instances",
        "pub struct Cell<T> { var value: T } pub enum Packet<T> { Data(T) } pub enum Unused<T> { Data(T) } fn get<T>(x: Cell<T>) -> T { x.value } fn main() -> (i32, bool, bool) { val a = Cell { value: 7 }; val b = Cell { value: true }; a.value = 8; (get(a), get(b), Packet::Data(7) == Packet::Data(7)) }",
        Expected::Value(Value::Tuple(vec![
            Value::I32(8),
            Value::Bool(true),
            Value::Bool(true),
        ])),
    );
    for route in Route::ALL {
        run(&generic_aggregates, route);
    }
    let checked_where_bounds = Case::new(
        "checked-where-bounds-through-forwarding",
        "trait Get { fn get(self) -> i32; } struct P {} impl Get for P { fn get(self) -> i32 { 42 } } fn read<T>(value: T) -> i32 where T: Get { value.get() } fn wrap<U>(value: U) -> i32 where U: Get { read(value) } fn pass<T>(value: T) -> T where T: HashKey { value } fn main() -> (i32, i32) { (wrap(P {}), pass(7)) }",
        Expected::Value(Value::Tuple(vec![Value::I32(42), Value::I32(7)])),
    );
    for route in Route::ALL {
        run(&checked_where_bounds, route);
    }
    let distinct_trait_methods = Case::new(
        "nominal-trait-methods-on-one-receiver",
        "trait Left { fn get(self) -> i32; } trait Right { fn get(self) -> i32; } struct Point {} impl Left for Point { fn get(self) -> i32 { 11 } } impl Right for Point { fn get(self) -> i32 { 22 } } fn left<T: Left>(x: T) -> i32 { x.get() } fn right<T: Right>(x: T) -> i32 { x.get() } fn main() -> (i32, i32) { val p = Point {}; (left(p), right(p)) }",
        Expected::Value(Value::Tuple(vec![Value::I32(11), Value::I32(22)])),
    );
    for route in Route::ALL {
        run(&distinct_trait_methods, route);
    }
    for case in [
        Case::new(
            "empty-type-application-is-not-erased",
            "fn unused(x: i32<>) {} fn main() {}",
            Expected::Diagnostic("KG_TYPE_UNKNOWN_TYPE"),
        ),
        Case::new(
            "local-type-shadows-standard-constructor",
            "struct Map {} fn unused(x: Map<i32, String>) {} fn main() {}",
            Expected::Diagnostic("KG_TYPE_UNKNOWN_TYPE"),
        ),
        Case::new(
            "binder-shadows-standard-type-constructor",
            "fn unused<Option>(x: Option<i32>) {} fn main() {}",
            Expected::Diagnostic("KG_TYPE_UNKNOWN_TYPE"),
        ),
        Case::new(
            "ambiguous-bound-method",
            "trait Left { fn get(self) -> i32; } trait Right { fn get(self) -> i32; } fn read<T: Left + Right>(x: T) -> i32 { x.get() } fn main() {}",
            Expected::Diagnostic("KG_TYPE_AMBIGUOUS_METHOD"),
        ),
        Case::new(
            "duplicate-trait-method",
            "trait View { fn get(self) -> i32; fn get(self) -> i32; } fn main() {}",
            Expected::Diagnostic("KG_RESOLVE_DUPLICATE_METHOD"),
        ),
        Case::new(
            "duplicate-impl-method",
            "struct Point {} impl Point { fn get(self) -> i32 { 1 } fn get(self) -> i32 { 2 } } fn main() {}",
            Expected::Diagnostic("KG_RESOLVE_DUPLICATE_METHOD"),
        ),
        Case::new(
            "applied-impl-trait-preserves-arguments",
            "trait View<T> {} struct Point {} impl View<i32> for Point {} fn main() {}",
            Expected::Value(Value::Unit),
        ),
        Case::new(
            "generic-binder-shadows-trait",
            "trait View {} struct Point {} impl<View> View for Point {} fn main() {}",
            Expected::Diagnostic("KG_TYPE_INVALID_TRAIT_REFERENCE"),
        ),
        Case::new(
            "standard-constraint-cannot-be-implemented",
            "struct Point {} impl HashKey for Point {} fn main() {}",
            Expected::Diagnostic("KG_TYPE_INVALID_TRAIT_REFERENCE"),
        ),
    ] {
        for route in Route::ALL {
            run(&case, route);
        }
    }
    for case in [
        Case::new("resolved-runtime-helpers", "struct Cell { var n: i32 } fn main() -> i32 { val c = Cell { n: 1 }; val xs = [1]; set_field(c, \"n\", 2); set_index(xs, 0, 3); print(type_of(7)); get_field(c, \"n\") + xs[0] }", Expected::Value(Value::I32(5))).effects(&["i32"], &["i32"]).reflection(),
        Case::new("shadowed-print-has-no-host-effect", "fn print(n: i32) -> i32 { n + 1 } fn main() -> i32 { print(6) }", Expected::Value(Value::I32(7))),
        Case::new("bare-function-is-not-return-value", "fn answer() -> i32 { 42 } fn main() -> i32 { answer }", Expected::Diagnostic("KG_TYPE_INVALID_VALUE_TARGET")),
        Case::new("bare-helper-is-not-a-value", "fn main() { val f = print; }", Expected::Diagnostic("KG_TYPE_INVALID_VALUE_TARGET")),
    ] {
        for route in Route::ALL { run(&case, route); }
    }
    for case in [
        Case::new(
            "ambiguous-module-name",
            "const pick: i32 = 1; fn pick() -> i32 { 2 } fn main() -> i32 { pick() }",
            Expected::Diagnostic("KG_RESOLVE_DUPLICATE_DECLARATION"),
        ),
        Case::new(
            "duplicate-constant",
            "const n: i32 = 1; const n: i32 = 2; fn main() -> i32 { n }",
            Expected::Diagnostic("KG_RESOLVE_DUPLICATE_DECLARATION"),
        ),
        Case::new(
            "shadowed-standard-namespace",
            "use std::math as api; fn main(api: i32) -> i32 { api::clamp(1, 1, 1) }",
            Expected::Diagnostic("KG_RESOLVE_UNKNOWN_NAME"),
        ),
        Case::new(
            "resolved-standard-call",
            "use std::math as api; fn main() -> i32 { api::clamp(5, 1, 3) + std::math::clamp(0, 2, 4) }",
            Expected::Value(Value::I32(5)),
        ),
    ] {
        for route in Route::ALL {
            run(&case, route);
        }
    }
    for case in [
        Case::new("enum-value-members", "enum Event { Empty, Data(i32, String) } fn main() -> bool { Event::Empty == Event::Empty() && Event::Data(7, \"x\") == Event::Data(7, \"x\") && Event::Data(7, \"x\") != Event::Data(8, \"x\") }", Expected::Value(Value::Bool(true))),
        Case::new("enum-alias-members", "enum Event { Data([i32]) } fn main() -> bool { val a = [1]; val x = Event::Data(a); a.push(2); x == Event::Data(a) && x != Event::Data([1, 2]) }", Expected::Value(Value::Bool(true))),
        Case::new("enum-evaluation-order", "enum Event { Data(i32, i32) } fn first() -> i32 { print(\"first\"); 1 } fn second() -> i32 { print(\"second\"); 2 } fn main() -> bool { Event::Data(first(), second()) == Event::Data(1, 2) }", Expected::Value(Value::Bool(true))).effects(&["first", "second"], &["first", "second"]),
    ] {
        for route in Route::ALL { run(&case, route); }
    }
    let mut budget_before_overflow = Case::new(
        "budget_before_overflow",
        "fn main() -> i32 { 2147483647 + 1 }",
        Expected::ResourceLimit,
    )
    .native();
    budget_before_overflow.max_steps = Some(2);
    let mut reject = Case::new(
        "host_reject",
        "fn main() { print(\"first\"); print(\"rejected\"); print(\"unreachable\"); }",
        Expected::HostFailure,
    )
    .effects(&["first", "rejected"], &["first"]);
    reject.reject_call = Some(2);
    let mut cached_init_failure = Case::new(
        "cached_init_failure",
        "print(\"init\"); val a = [1]; a[9]; fn main() {}",
        Expected::IndexTrap,
    )
    .effects(&["init"], &["init"]);
    cached_init_failure.repeat = 2;
    for case in [
        Case::new("heap-overflow-preserves-earlier-write", "use observe as test; fn main() { val a = test::array(); a[1] = 42; a[0] += 1; }", Expected::ScriptTrap("integer overflow")).array(&[2147483647, 0], &[2147483647, 42]),
        Case::new("heap-removed-target-not-recreated", "use observe as test; fn main() { val a = test::array(); a[0] += if true { a.clear(); print(\"removed\"); 2 } else { 0 }; }", Expected::IndexTrap).array(&[1], &[]).effects(&["removed"], &["removed"]),
        Case::new("heap-out-of-bounds-preserves-earlier-write", "use observe as test; fn main() { val a = test::array(); a[0] = 42; a[9] = 2; }", Expected::IndexTrap).array(&[1], &[42]),
        Case::new("heap-compound-reads-rhs-write", "use observe as test; fn main() { val a = test::array(); a[0] += if true { a[0] = 10; 2 } else { 0 }; }", Expected::Value(Value::Unit)).array(&[1], &[12]),
    ] {
        for route in Route::ALL { run(&case, route); }
    }
    let mut heap_reject = Case::new(
        "heap-host-rejection-preserves-earlier-write",
        "use observe as test; fn main() { val a = test::array(); a[0] = 42; print(\"committed\"); a[0] += if true { print(\"rejected\"); 2 } else { 0 }; }",
        Expected::HostFailure,
    ).array(&[1], &[42]).effects(&["committed", "rejected"], &["committed"]);
    heap_reject.reject_call = Some(3); // Array provider, committed log, rejected log.
    let mut heap_cancel = Case::new(
        "heap-cancellation-preserves-earlier-write",
        "use observe as test; fn main() { val a = test::array(); a[0] = 42; a[0] += if true { print(\"cancel\"); 2 } else { 0 }; print(\"unreachable\"); }",
        Expected::Cancelled,
    ).array(&[1], &[42]).effects(&["cancel"], &["cancel"]);
    heap_cancel.cancel_call = Some(2);
    let mut heap_budget = Case::new(
        "heap-budget-exhaustion-preserves-earlier-write",
        "use observe as test; fn main() { val a = test::array(); a[0] = 42; print(\"committed\"); a[0] += spin(); } fn spin() -> i32 { loop {} }",
        Expected::ResourceLimit,
    ).array(&[1], &[42]).effects(&["committed"], &["committed"]);
    heap_budget.max_steps = Some(100);
    for case in [heap_reject, heap_cancel, heap_budget] {
        for route in Route::ALL {
            run(&case, route);
        }
    }
    for operation in [
        "a.push(9)",
        "a.insert(a.len(), 9)",
        "a.pop()",
        "a.remove(a.len() - a.len())",
        "a.clear()",
    ] {
        let source = format!(
            "use observe as test; fn main() {{ val a = test::array(); a[0] = 42; print(\"before\"); {operation}; print(\"unreachable\"); }}"
        );
        let mut case = Case::new(
            "host-iteration-rejects-script-structural-write",
            &source,
            Expected::BuiltinTrap(format!(
                "std::array::{}: structural modification during iteration",
                operation
                    .strip_prefix("a.")
                    .unwrap()
                    .split('(')
                    .next()
                    .unwrap()
            )),
        )
        .array(&[1, 2], &[42, 2])
        .effects(&["before"], &["before"]);
        case.iterating = true;
        for route in Route::ALL {
            run(&case, route);
        }
    }
    let mut diamond = Case::new(
        "dependency-first-diamond-initializes-once",
        "use contract::left::left; use contract::right::right; print(\"root\"); fn main() -> i32 { left() + right() }",
        Expected::Value(Value::I32(42)),
    ).modules(&[
        ("leaf", "print(\"leaf\"); pub fn leaf() -> i32 { 21 }"),
        ("left", "use contract::leaf::leaf; print(\"left\"); pub fn left() -> i32 { leaf() }"),
        ("right", "use contract::leaf::leaf; print(\"right\"); pub fn right() -> i32 { leaf() }"),
    ]).effects(&["leaf", "left", "right", "root"], &["leaf", "left", "right", "root"]);
    diamond.repeat = 2;
    let mut failed_dependency = Case::new(
        "dependency-initialization-failure-is-cached",
        "use contract::dependency::answer; print(\"unreachable-root\"); fn main() -> i32 { answer() }",
        Expected::IndexTrap,
    ).modules(&[("dependency", "print(\"dependency\"); val a = [1]; a[9]; pub fn answer() -> i32 { 42 }")]).effects(&["dependency"], &["dependency"]);
    failed_dependency.repeat = 2;
    let cycle = Case::new(
        "cyclic-imports-prevent-execution",
        "use contract::dependency::answer; pub fn main() -> i32 { answer() }",
        Expected::ImportCycle,
    )
    .modules(&[(
        "dependency",
        "use contract::root::main; pub fn answer() -> i32 { main() }",
    )]);
    for case in [diamond, failed_dependency, cycle] {
        for route in Route::ALL {
            run(&case, route);
        }
    }
    for candidate in [
        Case::new(
            "candidate-external-effect",
            "print(\"forbidden\"); fn main() -> i32 { 9 }",
            Expected::CapabilityDenied,
        ),
        Case::new(
            "candidate-init-trap",
            "val a = [1]; a[9]; fn main() -> i32 { 9 }",
            Expected::IndexTrap,
        ),
        Case::new(
            "candidate-dependency-effect",
            "use contract::dependency::answer; fn main() -> i32 { answer() }",
            Expected::CapabilityDenied,
        )
        .modules(&[(
            "dependency",
            "print(\"forbidden-dependency\"); pub fn answer() -> i32 { 9 }",
        )]),
    ] {
        let mut case = Case::new(
            "failed-candidate-preserves-active-entry",
            "print(\"original-init\"); fn main() -> i32 { 42 }",
            Expected::Value(Value::I32(42)),
        )
        .effects(&["original-init"], &["original-init"]);
        if !candidate.modules.is_empty() {
            case.source = "use contract::dependency::answer; print(\"original-init\"); fn main() -> i32 { answer() }";
            case.modules = &[("dependency", "pub fn answer() -> i32 { 42 }")];
        }
        case.rejected_reload = Some(&candidate);
        for route in Route::ALL {
            run(&case, route);
        }
    }
    let candidate = Case::new(
        "published-dependency-version",
        "use contract::dependency::answer; fn main() -> i32 { answer() }",
        Expected::Value(Value::I32(99)),
    )
    .modules(&[(
        "dependency",
        "val isolated = [9]; pub fn answer() -> i32 { 99 }",
    )]);
    let mut versioned = Case::new(
        "publication-pins-old-dependencies-and-rejects-stale-candidate",
        "use contract::dependency::answer; print(\"old-init\"); fn main() -> i32 { answer() }",
        Expected::Value(Value::I32(42)),
    )
    .modules(&[("dependency", "pub fn answer() -> i32 { 42 }")])
    .effects(&["old-init"], &["old-init"]);
    versioned.published_reload = Some(&candidate);
    for route in Route::ALL {
        run(&versioned, route);
    }
    let cases = [
        Case::new("unknown-inherent-impl-target", "impl Missing {} fn main() {}", Expected::Diagnostic("KG_TYPE_UNKNOWN_ANNOTATION")),
        Case::new("unknown-where-target", "fn bad<T>(value: T) where Missing: Comparable {} fn main() {}", Expected::Diagnostic("KG_TYPE_INVALID_BOUND_TARGET")),
        Case::new("shadowed-generic-return", "impl<T> [T] { fn wrong<T>(self, value: T) -> T { self[0] } } fn main() {}", Expected::Diagnostic("KG_TYPE_RETURN_TYPE_MISMATCH")),
        Case::new("generic-values", "fn echo<T>(x: T) -> T { x } fn pass<U>(x: U) -> U { echo(x) } fn main() -> (i32, bool, String) { (pass(7), pass(true), echo(\"ok\")) }", Expected::Value(Value::Tuple(vec![Value::I32(7), Value::Bool(true), Value::Str("ok".into())]))),
        Case::new("generic-recursion", "fn repeat<T>(x: T, n: i32) -> T { if n == 0 { x } else { repeat(x, n - 1) } } fn main() -> i32 { repeat(7, 3) }", Expected::Value(Value::I32(7))),
        Case::new("generic-array-elements", "fn first<T>(xs: [T]) -> T { xs[0] } fn main() -> (i32, String) { (first([7]), first([\"ok\"])) }", Expected::Value(Value::Tuple(vec![Value::I32(7), Value::Str("ok".into())]))),
        Case::new("generic-equality", "fn same<T: Comparable>(a: T, b: T) -> bool { a == b } fn main() -> (bool, bool) { (same(1, 1), same(\"a\", \"b\")) }", Expected::Value(Value::Tuple(vec![Value::Bool(true), Value::Bool(false)]))),
        Case::new("generic-static-trait", "trait Get { fn get(self) -> i32; } struct P { val n: i32 } impl Get for P { fn get(self) -> i32 { self.n } } fn read<T: Get>(value: T) -> i32 { value.get() } fn wrap<U: Get>(value: U) -> i32 { read(value) } fn main() -> i32 { wrap(P { n: 42 }) }", Expected::Value(Value::I32(42))),
        Case::new("generic-impl-specialization", "trait Get { fn get(self) -> i32; } struct Holder<T> { val value: T } impl<T> Get for Holder<T> { fn get(self) -> i32 { 42 } } fn read<U: Get>(x: U) -> i32 { x.get() } fn main() -> (i32, i32) { (read(Holder { value: 1 }), read(Holder { value: \"a\" })) }", Expected::Value(Value::Tuple(vec![Value::I32(42), Value::I32(42)]))),
        Case::new("generic-impl-bound", "trait Get { fn get(self) -> i32; } struct Holder<T> { val value: T } impl<T: HashKey> Get for Holder<T> { fn get(self) -> i32 { 42 } } fn read<U: Get>(x: U) -> i32 { x.get() } fn main() -> i32 { read(Holder { value: 1 }) }", Expected::Value(Value::I32(42))),
        Case::new("generic-impl-bound-rejected", "trait Get { fn get(self) -> i32; } struct Holder<T> { val value: T } impl<T: HashKey> Get for Holder<T> { fn get(self) -> i32 { 42 } } fn read<U: Get>(x: U) -> i32 { x.get() } fn main() -> i32 { read(Holder { value: 1.5 }) }", Expected::Diagnostic("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED")),
        Case::new("generic-impl-trait-bound", "trait Key {} impl Key for i32 {} trait Get { fn get(self) -> i32; } struct Holder<T> { val value: T } impl<T: Key> Get for Holder<T> { fn get(self) -> i32 { 42 } } fn read<U: Get>(x: U) -> i32 { x.get() } fn main() -> i32 { read(Holder { value: 1 }) }", Expected::Value(Value::I32(42))),
        Case::new("generic-impl-trait-bound-rejected", "trait Key {} impl Key for i32 {} trait Get { fn get(self) -> i32; } struct Holder<T> { val value: T } impl<T: Key> Get for Holder<T> { fn get(self) -> i32 { 42 } } fn read<U: Get>(x: U) -> i32 { x.get() } fn main() -> i32 { read(Holder { value: \"a\" }) }", Expected::Diagnostic("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED")),
        Case::new("generic-impl-repeated-binder-rejected", "trait Get { fn get(self) -> i32; } struct Pair<T, U> { val left: T, val right: U } impl<T> Get for Pair<T, T> { fn get(self) -> i32 { 1 } } fn read<V: Get>(x: V) -> i32 { x.get() } fn main() -> i32 { read(Pair { left: 1, right: true }) }", Expected::Diagnostic("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED")),
        Case::new("generic-impl-overlap-rejected", "trait Get { fn get(self) -> i32; } struct Holder<T> { val value: T } impl<T> Get for Holder<T> { fn get(self) -> i32 { 1 } } impl Get for Holder<i32> { fn get(self) -> i32 { 2 } } fn main() {}", Expected::Diagnostic("KG_TYPE_INVALID_TRAIT_IMPL")),
        Case::new("applied-trait-impl-signature-rejected", "trait Echo<T> { fn get(self) -> T; } struct Pair { val number: i32 } impl Echo<i32> for Pair { fn get(self) -> String { \"bad\" } } fn main() {}", Expected::Diagnostic("KG_TYPE_TRAIT_METHOD_MISMATCH")),
        Case::new("applied-trait-impl-arity-rejected", "trait Echo<T> {} struct Pair {} impl Echo<i32, bool> for Pair {} fn main() {}", Expected::Diagnostic("KG_TYPE_INVALID_TRAIT_REFERENCE")),
        Case::new("applied-trait-impl-unknown-argument", "trait Echo<T> {} struct Pair {} impl Echo<Missing> for Pair {} fn main() {}", Expected::Diagnostic("KG_TYPE_UNKNOWN_ANNOTATION")),
        Case::new("applied-trait-impl-bound-rejected", "trait Echo<T: HashKey> {} struct Pair {} impl Echo<f32> for Pair {} fn main() {}", Expected::Diagnostic("KG_TYPE_STANDARD_CONSTRAINT_NOT_SATISFIED")),
        Case::new("applied-trait-impl-bound-accepted", "trait Echo<T: HashKey> {} struct Pair {} impl Echo<i32> for Pair {} fn main() {}", Expected::Value(Value::Unit)),
        Case::new("applied-trait-bound-dispatch", "trait Echo<T> { fn get(self) -> T; } struct Holder { val n: i32 } impl Echo<i32> for Holder { fn get(self) -> i32 { self.n } } fn read<U: Echo<i32>>(x: U) -> i32 { x.get() } fn main() -> i32 { read(Holder { n: 42 }) }", Expected::Value(Value::I32(42))),
        Case::new("applied-trait-where-bound-dispatch", "trait Echo<T> { fn get(self) -> T; } struct Holder { val n: i32 } impl Echo<i32> for Holder { fn get(self) -> i32 { self.n } } fn read<U>(x: U) -> i32 where U: Echo<i32> { x.get() } fn main() -> i32 { read(Holder { n: 42 }) }", Expected::Value(Value::I32(42))),
        Case::new("applied-trait-bound-distinguishes-arguments", "trait Echo<T> { fn get(self) -> T; } struct Holder { val n: i32 } impl Echo<i32> for Holder { fn get(self) -> i32 { self.n } } fn read<U: Echo<String>>(x: U) -> String { x.get() } fn main() -> String { read(Holder { n: 42 }) }", Expected::Diagnostic("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED")),
        Case::new("applied-trait-impls-share-receiver", "trait Echo<T> { fn get(self) -> T; } struct Holder {} impl Echo<i32> for Holder { fn get(self) -> i32 { 42 } } impl Echo<String> for Holder { fn get(self) -> String { \"ok\" } } fn read_int<U: Echo<i32>>(x: U) -> i32 { x.get() } fn read_string<U: Echo<String>>(x: U) -> String { x.get() } fn main() -> (i32, String) { (read_int(Holder {}), read_string(Holder {})) }", Expected::Value(Value::Tuple(vec![Value::I32(42), Value::Str("ok".into())]))),
        Case::new("applied-trait-template-specialization", "trait Echo<T> { fn get(self) -> T; } struct Holder<T> { val value: T } impl<T> Echo<T> for Holder<T> { fn get(self) -> T { self.value } } fn read<U: Echo<i32>>(x: U) -> i32 { x.get() } fn main() -> i32 { read(Holder { value: 42 }) }", Expected::Value(Value::I32(42))),
        Case::new("applied-trait-caller-binder", "trait Echo<T> { fn get(self) -> T; } struct Holder<T> { val value: T } impl<T> Echo<T> for Holder<T> { fn get(self) -> T { self.value } } fn read<T, U: Echo<T>>(x: U, fallback: T) -> T { x.get() } fn main() -> i32 { read(Holder { value: 42 }, 0) }", Expected::Value(Value::I32(42))),
        Case::new("applied-trait-template-overlap-rejected", "trait Echo<T> { fn get(self) -> T; } struct Holder<T> { val value: T } impl<T> Echo<T> for Holder<T> { fn get(self) -> T { self.value } } impl Echo<i32> for Holder<i32> { fn get(self) -> i32 { 1 } } fn main() {}", Expected::Diagnostic("KG_TYPE_INVALID_TRAIT_IMPL")),
        Case::new("generic-conflicting-arguments", "fn choose<T>(a: T, b: T) -> T { a } fn main() -> i32 { choose(1, true) }", Expected::Diagnostic("KG_TYPE_ARGUMENT_TYPE_MISMATCH")),
        Case::new("generic-missing-argument", "fn unused<T>() {} fn main() { unused(); }", Expected::Diagnostic("KG_TYPE_CANNOT_INFER_GENERIC_ARGUMENT")),
        Case::new("generic-public-entry", "pub fn echo<T>(value: T) -> T { value } fn main() {}", Expected::Diagnostic("KG_TYPE_PUBLIC_GENERIC_FUNCTION")),
        Case::new("generic-parameter-order", "fn reverse<T, U>(first: U, second: T) -> (T, U) { (second, first) } fn main() -> (i32, bool) { reverse(true, 7) }", Expected::Value(Value::Tuple(vec![Value::I32(7), Value::Bool(true)]))),
        Case::new("return-discards-following-effects", "fn main() -> i32 { return 7; print(\"unreachable\"); 42 }", Expected::Value(Value::I32(7))),
        Case::new("generic-numeric-instances", "fn add<T: OrderedNumber>(a: T, b: T) -> T { a + b } fn main() -> (i32, f32) { (add(1, 2), add(1.5, 2.5)) }", Expected::Value(Value::Tuple(vec![Value::I32(3), Value::F32(4.0)]))),
        Case::new("generic-same-storage-distinct-types", "trait Get { fn get(self) -> i32; } struct P { val n: i32 } struct Q { val n: i32 } impl Get for P { fn get(self) -> i32 { self.n } } impl Get for Q { fn get(self) -> i32 { self.n + 1 } } fn read<T: Get>(value: T) -> i32 { value.get() } fn main() -> (i32, i32) { (read(P { n: 42 }), read(Q { n: 42 })) }", Expected::Value(Value::Tuple(vec![Value::I32(42), Value::I32(43)]))),
        Case::new("generic-numeric-overflow", "fn add<T: OrderedNumber>(a: T, b: T) -> T { a + b } fn main() -> i32 { print(\"before\"); add(2147483647, 1) }", Expected::ScriptTrap("integer overflow")).effects(&["before"], &["before"]),
        Case::new("generic-missing-bound", "trait Get { fn get(self) -> i32; } fn read<T: Get>(value: T) -> i32 { value.get() } fn main() -> i32 { read(1) }", Expected::Diagnostic("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED")),
        Case::new("duplicate-field-declarations", "struct P { val x: i32, var x: i32 } fn main() {}", Expected::Diagnostic("KG_RESOLVE_DUPLICATE_FIELD")),
        Case::new("field-initializers-follow-source-order", "struct P { var left: i32, var right: i32 } fn left() -> i32 { print(\"left\"); 1 } fn right() -> i32 { print(\"right\"); 2 } fn main() -> i32 { val p = P { right: right(), left: left() }; p.left += p.right; p.left * 10 + p.right }", Expected::Value(Value::I32(32))).effects(&["right", "left"], &["right", "left"]),
        Case::new("explicit-string-lengths", "fn main() -> (usize, usize) { (\"中😀\".len_bytes(), \"中😀\".len_chars()) }", Expected::Value(Value::Tuple(vec![Value::I64(7), Value::I64(2)]))),
        Case::new("reject-obsolete-string-len", "fn main() { \"text\".len(); }", Expected::Diagnostic("KG_RESOLVE_UNKNOWN_NAME")),
        Case::new("iter-array-option", "fn main() -> (usize, i32, bool) { val a = [4, 7]; (std::iter::len(a), std::iter::get(a, \"a\".len_bytes()).unwrap_or(0), std::iter::get(a, a.len()).is_none()) }", Expected::Value(Value::Tuple(vec![Value::I64(2), Value::I32(7), Value::Bool(true)]))),
        Case::new("iter-string-option", "fn main() -> (usize, String, bool) { val s = \"中😀\"; (std::iter::len(s), std::iter::get(s, \"a\".len_bytes()).unwrap_or(\"missing\"), std::iter::get(s, s.len_chars()).is_none()) }", Expected::Value(Value::Tuple(vec![Value::I64(2), Value::Str("😀".into()), Value::Bool(true)]))),
        Case::new("pop-empty-option", "fn main() -> (i32, bool, usize) { val a = [7]; val alias = a; val popped = a.pop().unwrap_or(0); (popped, alias.pop().is_none(), a.len()) }", Expected::Value(Value::Tuple(vec![Value::I32(7), Value::Bool(true), Value::I64(0)]))),
        Case::new("user-print-is-direct-call", "fn print(n: i32) -> i32 { n + 1 } fn main() -> i32 { print(41) }", Expected::Value(Value::I32(42))),
        Case::new("user-type-of-is-direct-call", "fn type_of(n: i32) -> i32 { n + 2 } fn main() -> i32 { type_of(40) }", Expected::Value(Value::I32(42))),
        Case::new("local-print-is-not-a-helper", "fn main() { val print = 1; print(2); }", Expected::Diagnostic("KG_TYPE_INVALID_CALL_TARGET")),
        Case::new("method-receiver-before-argument", "fn receiver() -> [i32] { print(\"receiver\"); [1] } fn value() -> i32 { print(\"argument\"); 2 } fn main() { receiver().push(value()); }", Expected::Value(Value::Unit)).effects(&["receiver", "argument"], &["receiver", "argument"]),
        Case::new("compound-reads-current-local", "fn main() -> i32 { var n = 1; n += if true { n = 10; 2 } else { 0 }; n }", Expected::Value(Value::I32(12))),
        Case::new("compound-captures-index", "fn main() -> i32 { val a = [1, 2]; var i = 0; a[i] += if true { i = 1; 2 } else { 0 }; a[0] * 10 + a[1] }", Expected::Value(Value::I32(32))),
        Case::new("compound-keeps-root-identity", "fn main() -> i32 { var a = [1]; val old = a; a[0] += if true { a = [100]; 2 } else { 0 }; old[0] * 1000 + a[0] }", Expected::Value(Value::I32(3100))),
        Case::new("compound-reads-current-tuple", "fn main() -> i32 { var t = (1, 2); t[0] += if true { t = (10, 20); 2 } else { 0 }; t[0] + t[1] }", Expected::Value(Value::I32(32))),
        Case::new("local-compound-overflow", "fn main() -> i32 { var n = 2147483647; n += 1; n }", Expected::ScriptTrap("integer overflow")),
        Case::new("reject-assignment-index-type", "fn main() -> i32 { val a = [1]; a[true] += 1; a[0] }", Expected::Diagnostic("KG_TYPE_INVALID_ASSIGNMENT_TARGET")),
        Case::new("compound-scalars", "fn main() -> i32 { var n = 10; n += 5; n -= 3; n *= 2; n /= 4; n }", Expected::Value(Value::I32(6))),
        Case::new("assignment-evaluates-target-first", r#"
            fn root(a: [i32]) -> [i32] { print("root"); a }
            fn index() -> i32 { print("index"); 0 }
            fn rhs(a: [i32]) -> i32 { print("rhs"); a[0] = 20; 2 }
            fn main() -> i32 { val a = [1]; root(a)[index()] += rhs(a); a[0] }
        "#, Expected::Value(Value::I32(22))).effects(&["root", "index", "rhs"], &["root", "index", "rhs"]),
        Case::new("plain-assignment-evaluates-target-first", r#"
            fn root(a: [i32]) -> [i32] { print("root"); a }
            fn index() -> i32 { print("index"); 0 }
            fn rhs() -> i32 { print("rhs"); 42 }
            fn main() -> i32 { val a = [1]; root(a)[index()] = rhs(); a[0] }
        "#, Expected::Value(Value::I32(42))).effects(&["root", "index", "rhs"], &["root", "index", "rhs"]),
        Case::new("nested-location-evaluated-once", r#"
            struct Point { var x: i32 }
            fn index() -> i32 { print("index"); 0 }
            fn rhs(a: [Point]) -> i32 { print("rhs"); a[0] = Point { x: 20 }; 2 }
            fn main() -> i32 { val a = [Point { x: 1 }]; a[index()].x += rhs(a); a[0].x }
        "#, Expected::Value(Value::I32(22))).effects(&["index", "rhs"], &["index", "rhs"]),
        Case::new("rhs-removes-compound-target", r#"
            fn rhs(a: [i32]) -> i32 { a.pop(); print("removed"); 2 }
            fn main() -> i32 { val a = [1]; a[0] += rhs(a); print("written"); 0 }
        "#, Expected::IndexTrap).effects(&["removed"], &["removed"]),
        Case::new("rhs-repairs-missing-target", "fn rhs(a: [i32]) -> i32 { a.push(20); 2 } fn main() -> i32 { val a = [1]; a[1] += rhs(a); a[1] }", Expected::Value(Value::I32(22))),
        Case::new("compound-overflow", "fn main() -> i32 { val a = [2147483647]; a[0] += 1; a[0] }", Expected::ScriptTrap("integer overflow")),
        Case::new("tuple-copy-commit", "fn main() -> i32 { var t = ((1, 2), 3); val old = t; t[0][1] += 40; t[0][1] + old[0][1] }", Expected::Value(Value::I32(44))),
        Case::new("tuple-in-array-commit", "fn main() -> i32 { val a = [(1, 2)]; a[0][1] += 40; a[0][1] }", Expected::Value(Value::I32(42))),
        Case::new("rebind-array-slot", "fn main() -> i32 { var a = [1]; val old = a; a = [42]; a[0] + old[0] }", Expected::Value(Value::I32(43))),
        Case::new("reject-compound-val", "fn main() -> i32 { val n = 1; n += 1; n }", Expected::Diagnostic("KG_TYPE_INVALID_ASSIGNMENT_TARGET")),
        Case::new("reject-val-tuple-write", "fn main() -> i32 { val t = (1, 2); t[0] += 1; t[0] }", Expected::Diagnostic("KG_TYPE_INVALID_ASSIGNMENT_TARGET")),
        Case::new("reject-compound-bool", "fn main() -> bool { var n = true; n += false; n }", Expected::Diagnostic("KG_TYPE_BINARY_OPERAND_TYPE_MISMATCH")),
        Case::new("min-literal", "fn main() -> i32 { -2147483648 }", Expected::Value(Value::I32(i32::MIN))).native(),
        Case::new("const-min-literal", "const MIN: i32 = -2147483648; fn main() -> i32 { MIN }", Expected::Value(Value::I32(i32::MIN))).native(),
        Case::new("negate-min-literal", "fn main() -> i32 { -(-2147483648) }", Expected::ScriptTrap("integer overflow")).native(),
        Case::new("invalid-positive-literal", "fn main() -> i32 { 2147483648 }", Expected::Diagnostic("KG_TYPE_INVALID_LITERAL")),
        Case::new("invalid-negative-literal", "fn main() -> i32 { -2147483649 }", Expected::Diagnostic("KG_TYPE_INVALID_LITERAL")),
        Case::new("invalid-pattern-literal", "fn main() -> i32 { match 1 { 2147483648 => 10, _ => 20 } }", Expected::Diagnostic("KG_TYPE_INVALID_LITERAL")),
        Case::new("mismatched-pattern", "fn main() -> i32 { match true { 1 => 10, _ => 20 } }", Expected::Diagnostic("KG_TYPE_PATTERN_MISMATCH")),
        Case::new("checked-pattern-literal", "fn main() -> i32 { match 2147483647 { 2147483647 => 42, _ => 0 } }", Expected::Value(Value::I32(42))),
        Case::new("const-short-circuit",
            "const A: bool = false && (1 / 0 == 0); const B: bool = true || (2147483647 + 1 == 0); fn main() -> bool { !A && B }",
            Expected::Value(Value::Bool(true))),
        Case::new("const-dependency",
            "const BASE: i32 = 6 * 7; const NEXT: i32 = BASE + 1; fn main() -> i32 { NEXT }",
            Expected::Value(Value::I32(43))).native(),
        Case::new("const-overflow", "const BAD: i32 = 2147483647 + 1; fn main() -> i32 { BAD }",
            Expected::Diagnostic("KG_TYPE_INVALID_CONST_INITIALIZER")),
        Case::new("const-divide-zero", "const BAD: i32 = 1 / 0; fn main() -> i32 { BAD }",
            Expected::Diagnostic("KG_TYPE_INVALID_CONST_INITIALIZER")),
        Case::new("const-short-circuit-still-requires-const-safe-code",
            "const BAD: bool = true || effect(); fn effect() -> bool { print(\"no\"); true } fn main() -> bool { BAD }",
            Expected::Diagnostic("KG_TYPE_INVALID_CONST_INITIALIZER")),
        Case::new("add_overflow", "fn main() -> i32 { 2147483647 + 1 }", Expected::ScriptTrap("integer overflow")).native(),
        Case::new("temporary_overflow", "fn main() -> i32 { (2147483647 + 1) - 1 }", Expected::ScriptTrap("integer overflow")).native(),
        Case::new("sub_overflow", "fn main() -> i32 { (-2147483647 - 1) - 1 }", Expected::ScriptTrap("integer overflow")).native(),
        Case::new("mul_overflow", "fn main() -> i32 { 50000 * 50000 }", Expected::ScriptTrap("integer overflow")).native(),
        Case::new("neg_overflow", "fn main() -> i32 { -(-2147483647 - 1) }", Expected::ScriptTrap("integer overflow")).native(),
        Case::new("div_overflow", "fn main() -> i32 { (-2147483647 - 1) / -1 }", Expected::ScriptTrap("integer overflow")),
        Case::new("division_by_zero", "fn main() -> i32 { 1 / 0 }", Expected::ScriptTrap("integer division by zero")),
        Case::new("overflow_effects", "fn left() -> i32 { print(\"left\"); 2147483647 } fn right() -> i32 { print(\"right\"); 1 } fn main() -> i32 { left() + right() }", Expected::ScriptTrap("integer overflow")).effects(&["left", "right"], &["left", "right"]),
        budget_before_overflow,
        Case::new("scalar", "fn main() -> i32 { (2 + 3) * 4 }", Expected::Value(Value::I32(20))),
        Case::new("alias", "struct P { var n: i32 } fn main() -> i32 { val a = P { n: 1 }; val b = a; b.n = 7; a.n }", Expected::Value(Value::I32(7))),
        Case::new("object_identity", "struct P { var n: i32 } fn main() -> bool { val a = P { n: 1 }; val b = P { n: 1 }; a == b }", Expected::Value(Value::Bool(false))),
        Case::new("tuple_value", "fn main() -> bool { (1, \"a\") == (1, \"a\") }", Expected::Value(Value::Bool(true))),
        Case::new("short_circuit", "fn fail() -> bool { print(\"unreachable\"); val a = [1]; a[2] == 0 } fn main() -> bool { false && fail() }", Expected::Value(Value::Bool(false))),
        Case::new("left_to_right", "fn left() -> i32 { print(\"left\"); 1 } fn right() -> i32 { print(\"right\"); 2 } fn main() -> i32 { left() + right() }", Expected::Value(Value::I32(3))).effects(&["left", "right"], &["left", "right"]),
        Case::new("completed_effect_survives_trap", "fn main() { print(\"committed\"); val a = [1]; a[9] = 2; print(\"unreachable\"); }", Expected::IndexTrap).effects(&["committed"], &["committed"]),
        Case::new("const_rebind", "const N: i32 = 1; fn main() { N = 2; }", Expected::Diagnostic("KG_TYPE_INVALID_ASSIGNMENT_TARGET")),
        Case::new("missing_name", "fn main() { missing; }", Expected::Diagnostic("KG_RESOLVE_UNKNOWN_NAME")),
        Case::new("enum_value", "fn main() -> bool { val a = [1]; val b = [1]; a.pop() == b.pop() }", Expected::Value(Value::Bool(true))),
        Case::new("enum_different_members", "fn main() -> bool { val a = [1, 2]; a.pop() != a.pop() }", Expected::Value(Value::Bool(true))),
        Case::new("enum_object_identity", "fn main() -> bool { val a = [[1]]; val b = [[1]]; a.pop() != b.pop() }", Expected::Value(Value::Bool(true))),
        Case::new("enum_assert_eq", "fn main() { val a = [1]; val b = [1]; std::debug::assert_eq(a.pop(), b.pop(), \"same enum\"); }", Expected::Value(Value::Unit)),
        Case::new("shallow_copy", "struct P { var n: i32 } fn main() -> bool { val a = [P { n: 1 }]; val b = std::iter::to_array(a); b[0].n = 7; a != b && a[0].n == 7 }", Expected::Value(Value::Bool(true))),
        Case::new("map_alias_through_call", "fn change(value: Map<String, i32>) -> Map<String, i32> { value.insert(\"key\", 42); value } fn main() -> bool { val a: Map<String, i32> = std::map::new(); val b = change(a); val fresh: Map<String, i32> = std::map::new(); fresh.insert(\"key\", 42); a == b && a != fresh && a.get(\"key\") == b.get(\"key\") && a.len() == [0].len() }", Expected::Value(Value::Bool(true))),
        Case::new("set_alias_through_call", "fn change(value: Set<String>) -> Set<String> { value.insert(\"key\"); value } fn main() -> bool { val a: Set<String> = std::set::new(); val b = change(a); val fresh: Set<String> = std::set::new(); fresh.insert(\"key\"); a == b && a != fresh && a.contains(\"key\") }", Expected::Value(Value::Bool(true))),
        Case::new("map_values_are_shallow", "struct Item { var value: i32 } fn main() -> bool { val item = Item { value: 1 }; val a: Map<String, Item> = std::map::new(); a.insert(\"key\", item); val values = a.values(); values[0].value = 42; values.push(Item { value: 9 }); item.value == 42 && a.len() == [0].len() && values.len() == [0, 0].len() }", Expected::Value(Value::Bool(true))),
        Case::new("set_projection_has_independent_structure", "fn main() -> bool { val a: Set<String> = std::set::new(); a.insert(\"key\"); val values = a.to_array(); values[0] = \"changed\"; values.push(\"extra\"); a.contains(\"key\") && !a.contains(\"changed\") && a.len() == [0].len() && values.len() == [0, 0].len() }", Expected::Value(Value::Bool(true))),
        Case::new("enum_tuple_members_keep_map_identity", "enum Packet { Data((Map<String, i32>, i32)) } fn main() -> bool { val a: Map<String, i32> = std::map::new(); val b: Map<String, i32> = std::map::new(); val x = Packet::Data((a, 7)); val y = x; a.insert(\"key\", 42); b.insert(\"key\", 42); x == y && x == Packet::Data((a, 7)) && x != Packet::Data((b, 7)) }", Expected::Value(Value::Bool(true))),
        Case::new("interface_equality_rejected", "trait Marker {} fn same(a: Marker, b: Marker) -> bool { a == b }", Expected::Diagnostic("KG_TYPE_BINARY_OPERAND_TYPE_MISMATCH")),
        Case::new("tuple_interface_equality_rejected", "trait Marker {} fn same(a: Marker, b: Marker) -> bool { (1, a) == (1, b) }", Expected::Diagnostic("KG_TYPE_BINARY_OPERAND_TYPE_MISMATCH")),
        reject,
        cached_init_failure,
    ];
    for case in &cases {
        for route in Route::ALL {
            run(case, route);
        }
    }
}
