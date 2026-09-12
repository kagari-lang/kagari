//! One observable suite for source, artifacts and the existing JIT/fallback.
//! No bytecode layouts or arena IDs appear in the fixture expectations.
use std::sync::{Arc, Mutex};

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
    for attempt in 0..case.repeat {
        let outcome = match route {
            Route::Jit => {
                vm.execute_with_backend(&loaded, "main", &mut CraneliftBackend::for_host().unwrap())
            }
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
            (expected, actual) => panic!(
                "{} ({route:?}, attempt {attempt}): expected {expected:?}, got {actual:?}",
                case.name
            ),
        }
    }
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
