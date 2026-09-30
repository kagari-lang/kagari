use crate::{Vm, VmError, tests::common::compile_test_bytecode};
use kagari_abi::native_import::EngineNativeOperation;
use kagari_bytecode::KbcArtifact;
use kagari_common::host_interface::standard_log;
use kagari_runtime::{
    CapabilitySet, HostExposurePolicy, LanguageProfile, Runtime, RuntimeConfig, RuntimeErrorKind,
    SecurityContext, gc::GcHeapConfig, host::HostFunction, value::Value,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for custom in [false, true] {
        for empty in [false, true] {
            let source = if custom {
                format!("Counter{{value:0,end:{}}}", if empty { 0 } else { 3 })
            } else if empty {
                "{val values: ArrayList<i32> = []; values.iter()}".into()
            } else {
                "[0,1,2].iter()".into()
            };
            for (method, call, ty, expected) in [
                (
                    "count",
                    "source.count()",
                    "usize",
                    if empty { "0" } else { "3" },
                ),
                (
                    "fold",
                    "source.fold(4, |a,n| {print(\"callback\"); a+n})",
                    "i32",
                    if empty { "4" } else { "7" },
                ),
                (
                    "for_each",
                    "source.for_each(|n| {print(\"callback\");})",
                    "()",
                    "()",
                ),
                (
                    "any",
                    "source.any(|n| {print(\"callback\"); n==1})",
                    "bool",
                    if empty { "false" } else { "true" },
                ),
                (
                    "all",
                    "source.all(|n| {print(\"callback\"); n<1})",
                    "bool",
                    if empty { "true" } else { "false" },
                ),
                (
                    "find",
                    "source.find(|n| {print(\"callback\"); n==1})",
                    "Option<i32>",
                    if empty { "None" } else { "Some(1)" },
                ),
                (
                    "last",
                    "source.last()",
                    "Option<i32>",
                    if empty { "None" } else { "Some(2)" },
                ),
            ] {
                let name = format!("{method}_{custom}_{empty}");
                let source = format!(
                    r#"
struct Counter {{var value:i32,val end:i32}}
impl Iterator for Counter {{type Item=i32;fn next(self)->Option<i32>{{print("next");if self.value>=self.end {{None}}else{{val value=self.value;self.value+=1;Some(value)}}}}}}
fn consume<I:Iterator<Item=i32>>(source:I)->{ty} {{{call}}}
fn main()->i32 {{val result=consume({source});std::debug::assert_eq(result,{expected},"result");42}}
"#
                );
                cases.push((name, source));
            }
        }
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

// Observed at 5f6ca83 before replacing the compiler terminal algorithms.
struct Baseline {
    name: &'static str,
    steps: u64,
    effects: &'static [(&'static str, u64)],
}
const BASELINE: &[Baseline] = &[
    Baseline {
        name: "count_false_false",
        steps: 50,
        effects: &[],
    },
    Baseline {
        name: "fold_false_false",
        steps: 67,
        effects: &[("callback", 21), ("callback", 34), ("callback", 47)],
    },
    Baseline {
        name: "for_each_false_false",
        steps: 57,
        effects: &[("callback", 20), ("callback", 30), ("callback", 40)],
    },
    Baseline {
        name: "any_false_false",
        steps: 51,
        effects: &[("callback", 20), ("callback", 32)],
    },
    Baseline {
        name: "all_false_false",
        steps: 51,
        effects: &[("callback", 20), ("callback", 32)],
    },
    Baseline {
        name: "find_false_false",
        steps: 51,
        effects: &[("callback", 20), ("callback", 32)],
    },
    Baseline {
        name: "last_false_false",
        steps: 45,
        effects: &[],
    },
    Baseline {
        name: "count_false_true",
        steps: 25,
        effects: &[],
    },
    Baseline {
        name: "fold_false_true",
        steps: 27,
        effects: &[],
    },
    Baseline {
        name: "for_each_false_true",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "any_false_true",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "all_false_true",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "find_false_true",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "last_false_true",
        steps: 25,
        effects: &[],
    },
    Baseline {
        name: "count_true_false",
        steps: 118,
        effects: &[("next", 10), ("next", 39), ("next", 68), ("next", 97)],
    },
    Baseline {
        name: "fold_true_false",
        steps: 135,
        effects: &[
            ("next", 12),
            ("callback", 37),
            ("next", 46),
            ("callback", 71),
            ("next", 80),
            ("callback", 105),
            ("next", 114),
        ],
    },
    Baseline {
        name: "for_each_true_false",
        steps: 125,
        effects: &[
            ("next", 11),
            ("callback", 36),
            ("next", 42),
            ("callback", 67),
            ("next", 73),
            ("callback", 98),
            ("next", 104),
        ],
    },
    Baseline {
        name: "any_true_false",
        steps: 86,
        effects: &[
            ("next", 11),
            ("callback", 36),
            ("next", 44),
            ("callback", 69),
        ],
    },
    Baseline {
        name: "all_true_false",
        steps: 86,
        effects: &[
            ("next", 11),
            ("callback", 36),
            ("next", 44),
            ("callback", 69),
        ],
    },
    Baseline {
        name: "find_true_false",
        steps: 86,
        effects: &[
            ("next", 11),
            ("callback", 36),
            ("next", 44),
            ("callback", 69),
        ],
    },
    Baseline {
        name: "last_true_false",
        steps: 113,
        effects: &[("next", 10), ("next", 37), ("next", 64), ("next", 91)],
    },
    Baseline {
        name: "count_true_true",
        steps: 31,
        effects: &[("next", 10)],
    },
    Baseline {
        name: "fold_true_true",
        steps: 33,
        effects: &[("next", 12)],
    },
    Baseline {
        name: "for_each_true_true",
        steps: 32,
        effects: &[("next", 11)],
    },
    Baseline {
        name: "any_true_true",
        steps: 32,
        effects: &[("next", 11)],
    },
    Baseline {
        name: "all_true_true",
        steps: 32,
        effects: &[("next", 11)],
    },
    Baseline {
        name: "find_true_true",
        steps: 32,
        effects: &[("next", 11)],
    },
    Baseline {
        name: "last_true_true",
        steps: 31,
        effects: &[("next", 10)],
    },
];

#[test]
fn iterator_terminals_preserve_effects_and_every_budget_cut() {
    let cases = cases();
    assert_eq!(cases.len(), BASELINE.len());
    for (name, source) in cases {
        let baseline = BASELINE.iter().find(|case| case.name == name).unwrap();
        let program = compile_test_bytecode(&source);
        assert_eq!(
            program.modules[program.root.index()]
                .engine_imports
                .iter()
                .filter(|import| matches!(
                    import.resolve(),
                    Some(EngineNativeOperation::Resumable(_))
                ))
                .count(),
            1
        );
        for encoded in [false, true] {
            let program = if encoded {
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
            };
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            let base = Rc::new(Cell::new(0));
            let root_base = base.clone();
            let mut runtime = runtime();
            runtime
                .register_host_function(HostFunction::new(standard_log(), move |context, args| {
                    let Value::Str(label) = &args[0] else {
                        panic!("log label");
                    };
                    sink.borrow_mut().push((
                        label.clone(),
                        context.runtime().resources().counters().instruction_steps
                            - root_base.get(),
                    ));
                    Ok(Value::Unit)
                }))
                .unwrap();
            let loaded = runtime.load_program("iterator-native", program).unwrap();
            let mut vm = Vm::new(runtime);
            for limit in 0..=baseline.steps {
                effects.borrow_mut().clear();
                base.set(vm.runtime().resources().counters().instruction_steps);
                let mut options = vm.runtime().execution_options();
                options.resources.max_instruction_steps = Some(limit);
                let session = vm.runtime().begin_execution(&loaded, options).unwrap();
                let result = vm.execute(&loaded, "main");
                if limit == baseline.steps {
                    assert_eq!(result.unwrap().return_value, Value::I32(42), "{name}");
                } else {
                    assert!(
                        matches!(result,Err(VmError::RuntimeError(error)) if error.kind()==RuntimeErrorKind::ResourceLimitExceeded),
                        "{name} {limit} {encoded}"
                    );
                }
                assert_eq!(session.counters().instruction_steps, limit, "{name}");
                let expected: Vec<_> = baseline
                    .effects
                    .iter()
                    .filter(|(_, step)| *step <= limit)
                    .map(|(label, step)| (label.to_string(), *step))
                    .collect();
                assert_eq!(*effects.borrow(), expected, "{name} {limit} {encoded}");
                assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
                assert!(!vm.runtime().is_quarantined());
                // A cut before terminal entry can leave the newly created
                // iterator's direct-call guard alive until its root session ends.
                drop(session);
                assert_eq!(vm.runtime().gc().active_roots(), 0, "{name} {limit}");
                vm.runtime().collect_garbage().unwrap();
                assert_eq!(vm.runtime().gc().allocated_objects(), 0);
            }
        }
    }
}

#[test]
fn generic_protocol_targets_and_nested_heap_callbacks_stay_rooted() {
    for source in [
        r#"
struct Counter {var value:i32}
impl Iterator for Counter {
    type Item=i32;
    fn count(self)->usize {99}
    fn next(self)->Option<i32> {if self.value==0 {None}else{self.value=0;Some(42)}}
}
fn main()->i32 {val source=Counter{value:1};std::debug::assert_eq(source.count(),99,"override");source.fold(0,|a,n|a+n)}
"#,
        r#"
struct Counter<T> {val item:T,var done:bool}
impl<T> Iterator for Counter<T> {type Item=T;fn next(self)->Option<T>{if self.done {None}else{self.done=true;Some(self.item)}}}
fn count<I:Iterator>(source:I)->usize {source.count()}
fn main()->i32 {val source=Counter{item:[40,2],done:false};val result=source.fold([0],|a,n|[n[0]+n[1]+a[0]]);std::debug::assert_eq(count(source),0,"shared progress");result[0]}
"#,
        r#"
fn main()->i32 {
    var visits=0;
    val values=[20,22];
    val source=values.iter().map(|n|{visits+=1;[n]});
    val result=source.fold([0],|a,n|{val inner: Option<ArrayList<i32>> = None;inner.unwrap_or_else(||[a[0]+n[0]])});
    std::debug::assert_eq(visits,2,"once");values.push(1);result[0]
}
"#,
    ] {
        let program = compile_test_bytecode(source);
        for encoded in [false, true] {
            let program = if encoded {
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
            };
            let mut runtime = runtime();
            let loaded = runtime.load_program("iterator-native", program).unwrap();
            let mut vm = Vm::new(runtime);
            assert_eq!(
                vm.execute(&loaded, "main").unwrap().return_value,
                Value::I32(42)
            );
            assert_eq!(vm.runtime().gc().active_roots(), 0);
            assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
            assert!(!vm.runtime().is_quarantined());
            assert!(vm.runtime().gc().stats().collections > 0);
            vm.runtime().collect_garbage().unwrap();
            assert_eq!(vm.runtime().gc().allocated_objects(), 0);
        }
    }
}

#[test]
fn protocol_and_callback_traps_preserve_frames_and_release_guards() {
    for source in [
        r#"
struct Broken {}
impl Iterator for Broken {type Item=i32;fn next(self)->Option<i32>{val n=2147483647;Some(n+1)}}
fn main()->i32 {Broken{}.fold(0,|a,n|a+n)}
fn ready()->i32 {7}
"#,
        r#"
fn main()->i32 {val source=[1,2].iter();source.fold(0,|a,n|{val x=2147483647;x+1})}
fn ready()->i32 {7}
"#,
    ] {
        let mut runtime = runtime();
        let loaded = runtime
            .load_program("iterator-native", compile_test_bytecode(source))
            .unwrap();
        let mut vm = Vm::new(runtime);
        let Err(VmError::RuntimeError(error)) = vm.execute(&loaded, "main") else {
            panic!("expected trap");
        };
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        let trace = error.trace().unwrap();
        assert_eq!(trace.frames.len(), 2);
        assert_eq!(trace.frames[1].function_name, "main");
        assert!(
            trace.frames[0].function_name.contains("next")
                || trace.frames[0].function_name.contains("closure")
        );
        assert_eq!(vm.runtime().gc().active_roots(), 0);
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
        assert!(!vm.runtime().is_quarantined());
        assert_eq!(
            vm.execute(&loaded, "ready").unwrap().return_value,
            Value::I32(7)
        );
    }
}
