use crate::{error::VmError, tests::common::compile_test_bytecode, vm::Vm};
use kagari_abi::{
    callable::EngineNativeBinding, native_import::EngineNativeOperation, standard::RuntimePrimitive,
};
use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::host_interface::standard_log;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use {
    kagari_common::capability::CapabilitySet,
    kagari_runtime::{
        Runtime, RuntimeConfig,
        error::RuntimeErrorKind,
        gc::GcHeapConfig,
        host::HostFunction,
        security::{HostExposurePolicy, LanguageProfile, SecurityContext},
        value::Value,
    },
};

struct Case {
    name: String,
    source: String,
}

fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let mut add = |name: String, ty: &str, input: &str, call: &str, expected: &str| {
        cases.push(Case { name, source: format!("fn eager() -> i32 {{ print(\"eager\"); 9 }} fn error() -> String {{ print(\"eager\"); \"error\" }} fn main() -> i32 {{ val value: {ty} = {input}; val result = {call}; std::debug::assert_eq(result, {expected}, \"result\"); 42 }}") });
    };
    for (variant, input) in [("some", "Some(7)"), ("none", "None")] {
        let some = variant == "some";
        for (method, call, yes, no) in [
            (
                "map",
                "value.map(|n| { print(\"mapper\"); n + 1 })",
                "Some(8)",
                "None",
            ),
            (
                "and_then",
                "value.and_then(|n| { print(\"mapper\"); Some(n + 1) })",
                "Some(8)",
                "None",
            ),
            ("ok_or", "value.ok_or(error())", "Ok(7)", "Err(\"error\")"),
            (
                "ok_or_else",
                "value.ok_or_else(|| { print(\"fallback\"); \"error\" })",
                "Ok(7)",
                "Err(\"error\")",
            ),
            (
                "unwrap_or_else",
                "value.unwrap_or_else(|| { print(\"fallback\"); 9 })",
                "7",
                "9",
            ),
            (
                "or_else",
                "value.or_else(|| { print(\"fallback\"); Some(9) })",
                "Some(7)",
                "Some(9)",
            ),
            (
                "map_or",
                "value.map_or(eager(), |n| { print(\"mapper\"); n + 1 })",
                "8",
                "9",
            ),
            (
                "map_or_else",
                "value.map_or_else(|| { print(\"fallback\"); 9 }, |n| { print(\"mapper\"); n + 1 })",
                "8",
                "9",
            ),
            (
                "is_some_and",
                "value.is_some_and(|n| { print(\"predicate\"); n > 0 })",
                "true",
                "false",
            ),
            (
                "filter_keep",
                "value.filter(|n| { print(\"predicate\"); n > 0 })",
                "Some(7)",
                "None",
            ),
            (
                "filter_reject",
                "value.filter(|n| { print(\"predicate\"); n < 0 })",
                "None",
                "None",
            ),
        ] {
            add(
                format!("option_{method}_{variant}"),
                "Option<i32>",
                input,
                call,
                if some { yes } else { no },
            );
        }
    }
    for (variant, input) in [("ok", "Ok(7)"), ("err", "Err(\"old\")")] {
        let ok = variant == "ok";
        for (method, call, yes, no) in [
            (
                "map",
                "value.map(|n| { print(\"mapper\"); n + 1 })",
                "Ok(8)",
                "Err(\"old\")",
            ),
            (
                "map_err",
                "value.map_err(|e| { print(\"mapper\"); 9 })",
                "Ok(7)",
                "Err(9)",
            ),
            (
                "and_then",
                "value.and_then(|n| { print(\"mapper\"); Result<i32, String>::Ok(n + 1) })",
                "Ok(8)",
                "Err(\"old\")",
            ),
            (
                "unwrap_or_else",
                "value.unwrap_or_else(|e| { print(\"fallback\"); 9 })",
                "7",
                "9",
            ),
            (
                "or_else",
                "value.or_else(|e| { print(\"fallback\"); Result<i32, i32>::Ok(9) })",
                "Ok(7)",
                "Ok(9)",
            ),
            (
                "map_or",
                "value.map_or(eager(), |n| { print(\"mapper\"); n + 1 })",
                "8",
                "9",
            ),
            (
                "map_or_else",
                "value.map_or_else(|e| { print(\"fallback\"); 9 }, |n| { print(\"mapper\"); n + 1 })",
                "8",
                "9",
            ),
            (
                "is_ok_and",
                "value.is_ok_and(|n| { print(\"predicate\"); n > 0 })",
                "true",
                "false",
            ),
            (
                "is_err_and",
                "value.is_err_and(|e| { print(\"predicate\"); e == \"old\" })",
                "false",
                "true",
            ),
            ("ok", "value.ok()", "Some(7)", "None"),
            ("err", "value.err()", "None", "Some(\"old\")"),
        ] {
            add(
                format!("result_{method}_{variant}"),
                "Result<i32, String>",
                input,
                call,
                if ok { yes } else { no },
            );
        }
    }
    for (name, ty, input, call, expected) in [
        (
            "option_zip_both",
            "Option<i32>",
            "Some(7)",
            "value.zip(Some(9))",
            "Some((7, 9))",
        ),
        (
            "option_zip_left_none",
            "Option<i32>",
            "None",
            "value.zip(Some(9))",
            "None",
        ),
        (
            "option_zip_right_none",
            "Option<i32>",
            "Some(7)",
            "value.zip(Option<i32>::None)",
            "None",
        ),
        (
            "option_flatten_some",
            "Option<Option<i32>>",
            "Some(Some(7))",
            "value.flatten()",
            "Some(7)",
        ),
        (
            "option_flatten_inner_none",
            "Option<Option<i32>>",
            "Some(None)",
            "value.flatten()",
            "None",
        ),
        (
            "option_flatten_none",
            "Option<Option<i32>>",
            "None",
            "value.flatten()",
            "None",
        ),
        (
            "result_flatten_ok",
            "Result<Result<i32, String>, String>",
            "Ok(Ok(7))",
            "value.flatten()",
            "Ok(7)",
        ),
        (
            "result_flatten_inner_err",
            "Result<Result<i32, String>, String>",
            "Ok(Err(\"inner\"))",
            "value.flatten()",
            "Err(\"inner\")",
        ),
        (
            "result_flatten_err",
            "Result<Result<i32, String>, String>",
            "Err(\"outer\")",
            "value.flatten()",
            "Err(\"outer\")",
        ),
        (
            "option_transpose_ok",
            "Option<Result<i32, String>>",
            "Some(Ok(7))",
            "value.transpose()",
            "Ok(Some(7))",
        ),
        (
            "option_transpose_err",
            "Option<Result<i32, String>>",
            "Some(Err(\"inner\"))",
            "value.transpose()",
            "Err(\"inner\")",
        ),
        (
            "option_transpose_none",
            "Option<Result<i32, String>>",
            "None",
            "value.transpose()",
            "Ok(None)",
        ),
        (
            "result_transpose_some",
            "Result<Option<i32>, String>",
            "Ok(Some(7))",
            "value.transpose()",
            "Some(Ok(7))",
        ),
        (
            "result_transpose_none",
            "Result<Option<i32>, String>",
            "Ok(None)",
            "value.transpose()",
            "None",
        ),
        (
            "result_transpose_err",
            "Result<Option<i32>, String>",
            "Err(\"outer\")",
            "value.transpose()",
            "Some(Err(\"outer\"))",
        ),
    ] {
        add(name.into(), ty, input, call, expected);
    }
    cases
}

fn runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
        },
        security: SecurityContext {
            profile: LanguageProfile {
                allow_host_calls: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..Default::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allowed_host_functions: vec!["host.log".into()],
            ..Default::default()
        },
        ..Default::default()
    })
}

// Observed at 0257db5 before replacing compiler-owned enum algorithms.
struct Baseline {
    name: &'static str,
    steps: u64,
    effects: &'static [(&'static str, u64)],
}
const BASELINE: &[Baseline] = &[
    Baseline {
        name: "option_map_some",
        steps: 27,
        effects: &[("mapper", 11)],
    },
    Baseline {
        name: "option_and_then_some",
        steps: 27,
        effects: &[("mapper", 11)],
    },
    Baseline {
        name: "option_ok_or_some",
        steps: 24,
        effects: &[("eager", 7)],
    },
    Baseline {
        name: "option_ok_or_else_some",
        steps: 20,
        effects: &[],
    },
    Baseline {
        name: "option_unwrap_or_else_some",
        steps: 18,
        effects: &[],
    },
    Baseline {
        name: "option_or_else_some",
        steps: 20,
        effects: &[],
    },
    Baseline {
        name: "option_map_or_some",
        steps: 30,
        effects: &[("eager", 7), ("mapper", 16)],
    },
    Baseline {
        name: "option_map_or_else_some",
        steps: 26,
        effects: &[("mapper", 12)],
    },
    Baseline {
        name: "option_is_some_and_some",
        steps: 25,
        effects: &[("predicate", 11)],
    },
    Baseline {
        name: "option_filter_keep_some",
        steps: 29,
        effects: &[("predicate", 11)],
    },
    Baseline {
        name: "option_filter_reject_some",
        steps: 29,
        effects: &[("predicate", 11)],
    },
    Baseline {
        name: "option_map_none",
        steps: 17,
        effects: &[],
    },
    Baseline {
        name: "option_and_then_none",
        steps: 17,
        effects: &[],
    },
    Baseline {
        name: "option_ok_or_none",
        steps: 22,
        effects: &[("eager", 6)],
    },
    Baseline {
        name: "option_ok_or_else_none",
        steps: 23,
        effects: &[("fallback", 9)],
    },
    Baseline {
        name: "option_unwrap_or_else_none",
        steps: 21,
        effects: &[("fallback", 9)],
    },
    Baseline {
        name: "option_or_else_none",
        steps: 23,
        effects: &[("fallback", 9)],
    },
    Baseline {
        name: "option_map_or_none",
        steps: 21,
        effects: &[("eager", 6)],
    },
    Baseline {
        name: "option_map_or_else_none",
        steps: 22,
        effects: &[("fallback", 10)],
    },
    Baseline {
        name: "option_is_some_and_none",
        steps: 17,
        effects: &[],
    },
    Baseline {
        name: "option_filter_keep_none",
        steps: 16,
        effects: &[],
    },
    Baseline {
        name: "option_filter_reject_none",
        steps: 16,
        effects: &[],
    },
    Baseline {
        name: "result_map_ok",
        steps: 27,
        effects: &[("mapper", 11)],
    },
    Baseline {
        name: "result_map_err_ok",
        steps: 20,
        effects: &[],
    },
    Baseline {
        name: "result_and_then_ok",
        steps: 27,
        effects: &[("mapper", 11)],
    },
    Baseline {
        name: "result_unwrap_or_else_ok",
        steps: 18,
        effects: &[],
    },
    Baseline {
        name: "result_or_else_ok",
        steps: 20,
        effects: &[],
    },
    Baseline {
        name: "result_map_or_ok",
        steps: 30,
        effects: &[("eager", 7), ("mapper", 16)],
    },
    Baseline {
        name: "result_map_or_else_ok",
        steps: 26,
        effects: &[("mapper", 12)],
    },
    Baseline {
        name: "result_is_ok_and_ok",
        steps: 25,
        effects: &[("predicate", 11)],
    },
    Baseline {
        name: "result_is_err_and_ok",
        steps: 19,
        effects: &[],
    },
    Baseline {
        name: "result_ok_ok",
        steps: 19,
        effects: &[],
    },
    Baseline {
        name: "result_err_ok",
        steps: 18,
        effects: &[],
    },
    Baseline {
        name: "result_map_err",
        steps: 20,
        effects: &[],
    },
    Baseline {
        name: "result_map_err_err",
        steps: 25,
        effects: &[("mapper", 11)],
    },
    Baseline {
        name: "result_and_then_err",
        steps: 20,
        effects: &[],
    },
    Baseline {
        name: "result_unwrap_or_else_err",
        steps: 23,
        effects: &[("fallback", 11)],
    },
    Baseline {
        name: "result_or_else_err",
        steps: 25,
        effects: &[("fallback", 11)],
    },
    Baseline {
        name: "result_map_or_err",
        steps: 23,
        effects: &[("eager", 7)],
    },
    Baseline {
        name: "result_map_or_else_err",
        steps: 24,
        effects: &[("fallback", 12)],
    },
    Baseline {
        name: "result_is_ok_and_err",
        steps: 19,
        effects: &[],
    },
    Baseline {
        name: "result_is_err_and_err",
        steps: 25,
        effects: &[("predicate", 11)],
    },
    Baseline {
        name: "result_ok_err",
        steps: 18,
        effects: &[],
    },
    Baseline {
        name: "result_err_err",
        steps: 19,
        effects: &[],
    },
    Baseline {
        name: "option_zip_both",
        steps: 29,
        effects: &[],
    },
    Baseline {
        name: "option_zip_left_none",
        steps: 18,
        effects: &[],
    },
    Baseline {
        name: "option_zip_right_none",
        steps: 23,
        effects: &[],
    },
    Baseline {
        name: "option_flatten_some",
        steps: 19,
        effects: &[],
    },
    Baseline {
        name: "option_flatten_inner_none",
        steps: 17,
        effects: &[],
    },
    Baseline {
        name: "option_flatten_none",
        steps: 16,
        effects: &[],
    },
    Baseline {
        name: "result_flatten_ok",
        steps: 19,
        effects: &[],
    },
    Baseline {
        name: "result_flatten_inner_err",
        steps: 19,
        effects: &[],
    },
    Baseline {
        name: "result_flatten_err",
        steps: 19,
        effects: &[],
    },
    Baseline {
        name: "option_transpose_ok",
        steps: 27,
        effects: &[],
    },
    Baseline {
        name: "option_transpose_err",
        steps: 25,
        effects: &[],
    },
    Baseline {
        name: "option_transpose_none",
        steps: 18,
        effects: &[],
    },
    Baseline {
        name: "result_transpose_some",
        steps: 27,
        effects: &[],
    },
    Baseline {
        name: "result_transpose_none",
        steps: 22,
        effects: &[],
    },
    Baseline {
        name: "result_transpose_err",
        steps: 21,
        effects: &[],
    },
];

fn route(program: &BytecodeProgram, encoded: bool) -> BytecodeProgram {
    if encoded {
        KbcArtifact::from_bytes(
            &KbcArtifact::from_program(program.clone(), Default::default())
                .unwrap()
                .to_bytes()
                .unwrap(),
        )
        .unwrap()
        .program
    } else {
        program.clone()
    }
}

#[test]
fn enum_families_preserve_results_effect_positions_and_every_budget_cut() {
    let cases = cases();
    assert_eq!(cases.len(), BASELINE.len());
    for case in cases {
        let baseline = BASELINE
            .iter()
            .find(|baseline| baseline.name == case.name)
            .unwrap();
        let steps = baseline.steps;
        let expected_effects = baseline.effects;
        let program = compile_test_bytecode(&case.source);
        assert_eq!(
            program.modules[program.root.index()]
                .native_imports
                .iter()
                .filter(|import| matches!(
                    import.resolve(),
                    Some(EngineNativeOperation::Resumable(EngineNativeBinding::Intrinsic(operation)))
                        if operation != RuntimePrimitive::AssertEq
                ))
                .count(),
            1,
            "{} must select its native binding",
            case.name
        );
        for encoded in [false, true] {
            let mut runtime = runtime();
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            let base = Rc::new(Cell::new(0));
            let root_base = base.clone();
            runtime
                .register_host_function(HostFunction::new(standard_log(), move |context, args| {
                    let Value::Str(label) = &args[0] else {
                        panic!("log label")
                    };
                    sink.borrow_mut().push((
                        label.clone(),
                        context.runtime().resources().counters().instruction_steps
                            - root_base.get(),
                    ));
                    Ok(Value::Unit)
                }))
                .unwrap();
            let loaded = runtime
                .load_program("enum-native", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(runtime);
            for limit in 0..=steps {
                effects.borrow_mut().clear();
                base.set(vm.runtime().resources().counters().instruction_steps);
                let mut options = vm.runtime().execution_options();
                options.resources.max_instruction_steps = Some(limit);
                let session = vm.runtime().begin_execution(&loaded, options).unwrap();
                let result = vm.execute(&loaded, "main");
                if limit == steps {
                    assert_eq!(
                        result.unwrap().return_value,
                        Value::I32(42),
                        "{} encoded {encoded}",
                        case.name
                    );
                } else {
                    assert!(
                        matches!(result, Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded),
                        "{} at {limit}, encoded {encoded}",
                        case.name
                    );
                }
                assert_eq!(session.counters().instruction_steps, limit, "{}", case.name);
                let expected: Vec<_> = expected_effects
                    .iter()
                    .filter(|(_, step)| *step <= limit)
                    .map(|(label, step)| (label.to_string(), *step))
                    .collect();
                assert_eq!(
                    *effects.borrow(),
                    expected,
                    "{} at {limit}, encoded {encoded}",
                    case.name
                );
                assert_eq!(vm.runtime().gc().active_roots(), 0, "{}", case.name);
                assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
                assert!(!vm.runtime().is_quarantined());
                drop(session);
                vm.runtime().collect_garbage().unwrap();
                assert_eq!(vm.runtime().gc().allocated_objects(), 0);
            }
        }
    }
}
