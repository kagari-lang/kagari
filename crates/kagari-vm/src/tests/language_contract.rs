//! One observable suite for source, artifacts and the existing JIT/fallback.
//! No bytecode layouts or arena IDs appear in the fixture expectations.
use std::{
    cell::Cell,
    sync::{Arc, Mutex},
};

use kagari_common::SourceFile;
use kagari_hir::{LanguageFeatureProfile, analyze_source};
use kagari_ir::{
    bytecode::{ArtifactBuildOptions, ArtifactCompatibility, KbcArtifact, lower_to_bytecode},
    lower_to_ir,
};
use kagari_jit_cranelift::CraneliftBackend;
use kagari_runtime::{
    CapabilitySet, HostExposurePolicy, LanguageProfile, Runtime, RuntimeConfig, SecurityContext,
    host::{HostError, HostFunction, HostParameter, HostPassingStyle},
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
}

#[derive(Debug)]
enum Expected {
    Value(Value),
    Diagnostic(&'static str),
    IndexTrap,
    HostFailure,
    ScriptTrap(&'static str),
    ResourceLimit,
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

struct Case {
    name: &'static str,
    source: &'static str,
    expected: Expected,
    calls: &'static [&'static str],
    committed: &'static [&'static str],
    reject_call: Option<usize>,
    repeat: usize,
    require_native: bool,
    max_steps: Option<u64>,
}

impl Case {
    fn new(name: &'static str, source: &'static str, expected: Expected) -> Self {
        Self {
            name,
            source,
            expected,
            calls: &[],
            committed: &[],
            reject_call: None,
            repeat: 1,
            require_native: false,
            max_steps: None,
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

    fn native(mut self) -> Self {
        self.require_native = true;
        self
    }
}

fn run(case: &Case, route: Route) {
    let source = SourceFile::new(case.name, case.source);
    let analyzed = analyze_source(
        &source,
        LanguageFeatureProfile {
            allow_host_calls: true,
            ..Default::default()
        },
    );
    if let Expected::Diagnostic(code) = case.expected {
        assert!(
            analyzed.diagnostics().iter().any(|d| d.kind.code() == code),
            "{} ({route:?}): {:?}",
            case.name,
            analyzed.diagnostics()
        );
        assert!(
            analyzed.into_codegen().is_err(),
            "diagnostic must prevent all execution routes"
        );
        return;
    }
    let analyzed = analyzed
        .into_codegen()
        .unwrap_or_else(|d| panic!("{}: {d:?}", case.name));
    let compiled = lower_to_bytecode(&lower_to_ir(&analyzed).unwrap()).unwrap();
    let module = match route {
        Route::Source | Route::Jit => compiled,
        Route::Artifact => {
            let bytes = KbcArtifact::from_module(compiled, ArtifactBuildOptions::default())
                .to_bytes()
                .unwrap();
            let decoded = KbcArtifact::from_bytes(&bytes).unwrap();
            decoded
                .validate_for_loader(&ArtifactCompatibility::default())
                .unwrap();
            decoded.module
        }
    };
    let mut runtime = Runtime::new(RuntimeConfig {
        resources: kagari_runtime::ResourcePolicy {
            max_instruction_steps: case.max_steps,
            ..Default::default()
        },
        security: SecurityContext {
            profile: LanguageProfile {
                allow_jit: true,
                allow_host_calls: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                jit: true,
                host_calls: true,
                ..Default::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allowed_host_functions: vec!["host.log".into()],
            ..Default::default()
        },
        ..Default::default()
    });
    let host = Arc::new(Mutex::new(RecordingHost::default()));
    let capture = host.clone();
    let reject_call = case.reject_call;
    runtime
        .register_host_function(HostFunction::new(
            "host.log",
            vec![HostParameter {
                name: "message",
                type_name: "String",
                passing: HostPassingStyle::SharedBorrow,
            }],
            "()",
            move |args| {
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
                Ok(Value::Unit)
            },
        ))
        .unwrap();
    let loaded = runtime.load_module(case.name, module).unwrap();
    let mut vm = Vm::new(runtime);
    let mut backend = RecordingBackend {
        inner: CraneliftBackend::for_host().unwrap(),
        invocations: Cell::new(0),
    };
    for attempt in 0..case.repeat {
        let outcome = match route {
            Route::Jit => vm.execute_with_backend(&loaded, "main", &mut backend),
            _ => vm.execute(&loaded, "main"),
        };
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
            (Expected::ResourceLimit, Err(VmError::RuntimeError(error)))
                if error.kind() == kagari_runtime::RuntimeErrorKind::ResourceLimitExceeded => {}
            (expected, actual) => panic!(
                "{} ({route:?}, attempt {attempt}): expected {expected:?}, got {actual:?}",
                case.name
            ),
        }
    }
    if matches!(route, Route::Jit) && case.require_native {
        assert_eq!(
            backend.invocations.get(),
            case.repeat,
            "{} must actually invoke native code",
            case.name
        );
    }
    assert_eq!(
        vm.runtime().resources().counters().current_call_depth,
        0,
        "{} ({route:?}): call resources must be released after success or failure",
        case.name
    );
    let host = host.lock().unwrap();
    let expected_calls = case
        .calls
        .iter()
        .map(|message| HostCall {
            symbol: "host.log",
            args: vec![Value::Str((*message).into())],
        })
        .collect::<Vec<_>>();
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
    let cases = [
        Case::new("explicit-string-lengths", "fn main() -> (usize, usize) { (\"中😀\".len_bytes(), \"中😀\".len_chars()) }", Expected::Value(Value::Tuple(vec![Value::I64(7), Value::I64(2)]))),
        Case::new("reject-obsolete-string-len", "fn main() { \"text\".len(); }", Expected::Diagnostic("KG_RESOLVE_UNKNOWN_NAME")),
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
        Case::new("interface_equality_rejected", "trait Marker {} fn same(a: Marker, b: Marker) -> bool { a == b }", Expected::Diagnostic("KG_TYPE_BINARY_OPERAND_TYPE_MISMATCH")),
        Case::new("tuple_interface_equality_rejected", "trait Marker {} fn same(a: Marker, b: Marker) -> bool { (1, a) == (1, b) }", Expected::Diagnostic("KG_TYPE_BINARY_OPERAND_TYPE_MISMATCH")),
        reject,
        cached_init_failure,
    ];
    for case in &cases {
        for route in [Route::Source, Route::Artifact, Route::Jit] {
            run(case, route);
        }
    }
}
