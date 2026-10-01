use crate::{error::VmError, reentry::reenter, tests::common::compile_test_bytecode, vm::Vm};

use kagari_abi::{callable::EngineNativeBinding, native_import::EngineNativeOperation};
use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::{cancellation::CancellationToken, host_interface::standard_log};
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

fn cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for custom in [false, true] {
        for length in [0, 1, 3] {
            let source = if custom {
                format!("Counter{{value:0,end:{length}}}")
            } else {
                format!(
                    "{{val values: ArrayList<i32> = {}; values.iter()}}",
                    match length {
                        0 => "[]",
                        1 => "[0]",
                        _ => "[0,1,2]",
                    }
                )
            };
            for (method, call, ty, expected) in [
                (
                    "find_map",
                    "source.find_map(|n| {print(\"callback\");if n==1 {Some(n+40)}else{None}})",
                    "Option<i32>",
                    if length == 3 { "Some(41)" } else { "None" },
                ),
                (
                    "find_map_miss",
                    "source.find_map(|n| {print(\"callback\");val out:Option<i32> = None;out})",
                    "Option<i32>",
                    "None",
                ),
                (
                    "position",
                    "source.position(|n| {print(\"callback\");n==1})",
                    "Option<usize>",
                    if length == 3 { "Some(1)" } else { "None" },
                ),
                (
                    "position_miss",
                    "source.position(|n| {print(\"callback\");false})",
                    "Option<usize>",
                    "None",
                ),
                (
                    "nth",
                    "source.nth(1)",
                    "Option<i32>",
                    if length == 3 { "Some(1)" } else { "None" },
                ),
                (
                    "nth_zero",
                    "source.nth(0)",
                    "Option<i32>",
                    if length == 0 { "None" } else { "Some(0)" },
                ),
                ("nth_miss", "source.nth(4)", "Option<i32>", "None"),
                (
                    "reduce",
                    "source.reduce(|a,n| {print(\"callback\");a+n})",
                    "Option<i32>",
                    match length {
                        0 => "None",
                        1 => "Some(0)",
                        _ => "Some(3)",
                    },
                ),
                (
                    "min_by",
                    "source.min_by(|a,n| {print(\"callback\");if a==0 {Ordering::Greater}else{Ordering::Equal}})",
                    "Option<i32>",
                    match length {
                        0 => "None",
                        1 => "Some(0)",
                        _ => "Some(1)",
                    },
                ),
                (
                    "max_by",
                    "source.max_by(|a,n| {print(\"callback\");if a==0 {Ordering::Less}else{Ordering::Equal}})",
                    "Option<i32>",
                    match length {
                        0 => "None",
                        1 => "Some(0)",
                        _ => "Some(2)",
                    },
                ),
                (
                    "max_by_keep",
                    "source.max_by(|a,n| {print(\"callback\");Ordering::Greater})",
                    "Option<i32>",
                    if length == 0 { "None" } else { "Some(0)" },
                ),
                (
                    "min_by_keep",
                    "source.min_by(|a,n| {print(\"callback\");Ordering::Less})",
                    "Option<i32>",
                    if length == 0 { "None" } else { "Some(0)" },
                ),
            ] {
                let name = format!("{method}_{custom}_{length}");
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

struct Baseline {
    name: &'static str,
    steps: u64,
    effects: &'static [(&'static str, u64)],
}
// Observed at e232804 before replacing these compiler terminal algorithms.
const BASELINE: &[Baseline] = &[
    Baseline {
        name: "find_map_false_0",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "find_map_miss_false_0",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "position_false_0",
        steps: 28,
        effects: &[],
    },
    Baseline {
        name: "position_miss_false_0",
        steps: 28,
        effects: &[],
    },
    Baseline {
        name: "nth_false_0",
        steps: 27,
        effects: &[],
    },
    Baseline {
        name: "nth_zero_false_0",
        steps: 27,
        effects: &[],
    },
    Baseline {
        name: "nth_miss_false_0",
        steps: 27,
        effects: &[],
    },
    Baseline {
        name: "reduce_false_0",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "min_by_false_0",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "max_by_false_0",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "max_by_keep_false_0",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "min_by_keep_false_0",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "find_map_false_1",
        steps: 45,
        effects: &[("callback", 20)],
    },
    Baseline {
        name: "find_map_miss_false_1",
        steps: 41,
        effects: &[("callback", 20)],
    },
    Baseline {
        name: "position_false_1",
        steps: 45,
        effects: &[("callback", 22)],
    },
    Baseline {
        name: "position_miss_false_1",
        steps: 43,
        effects: &[("callback", 22)],
    },
    Baseline {
        name: "nth_false_1",
        steps: 39,
        effects: &[],
    },
    Baseline {
        name: "nth_zero_false_1",
        steps: 35,
        effects: &[],
    },
    Baseline {
        name: "nth_miss_false_1",
        steps: 39,
        effects: &[],
    },
    Baseline {
        name: "reduce_false_1",
        steps: 36,
        effects: &[],
    },
    Baseline {
        name: "min_by_false_1",
        steps: 36,
        effects: &[],
    },
    Baseline {
        name: "max_by_false_1",
        steps: 36,
        effects: &[],
    },
    Baseline {
        name: "max_by_keep_false_1",
        steps: 36,
        effects: &[],
    },
    Baseline {
        name: "min_by_keep_false_1",
        steps: 36,
        effects: &[],
    },
    Baseline {
        name: "find_map_false_3",
        steps: 66,
        effects: &[("callback", 22), ("callback", 40)],
    },
    Baseline {
        name: "find_map_miss_false_3",
        steps: 71,
        effects: &[("callback", 22), ("callback", 36), ("callback", 50)],
    },
    Baseline {
        name: "position_false_3",
        steps: 60,
        effects: &[("callback", 24), ("callback", 40)],
    },
    Baseline {
        name: "position_miss_false_3",
        steps: 73,
        effects: &[("callback", 24), ("callback", 38), ("callback", 52)],
    },
    Baseline {
        name: "nth_false_3",
        steps: 48,
        effects: &[],
    },
    Baseline {
        name: "nth_zero_false_3",
        steps: 37,
        effects: &[],
    },
    Baseline {
        name: "nth_miss_false_3",
        steps: 63,
        effects: &[],
    },
    Baseline {
        name: "reduce_false_3",
        steps: 72,
        effects: &[("callback", 33), ("callback", 50)],
    },
    Baseline {
        name: "min_by_false_3",
        steps: 80,
        effects: &[("callback", 33), ("callback", 55)],
    },
    Baseline {
        name: "max_by_false_3",
        steps: 82,
        effects: &[("callback", 33), ("callback", 55)],
    },
    Baseline {
        name: "max_by_keep_false_3",
        steps: 66,
        effects: &[("callback", 33), ("callback", 47)],
    },
    Baseline {
        name: "min_by_keep_false_3",
        steps: 66,
        effects: &[("callback", 33), ("callback", 47)],
    },
    Baseline {
        name: "find_map_true_0",
        steps: 32,
        effects: &[("next", 11)],
    },
    Baseline {
        name: "find_map_miss_true_0",
        steps: 32,
        effects: &[("next", 11)],
    },
    Baseline {
        name: "position_true_0",
        steps: 34,
        effects: &[("next", 13)],
    },
    Baseline {
        name: "position_miss_true_0",
        steps: 34,
        effects: &[("next", 13)],
    },
    Baseline {
        name: "nth_true_0",
        steps: 33,
        effects: &[("next", 12)],
    },
    Baseline {
        name: "nth_zero_true_0",
        steps: 33,
        effects: &[("next", 12)],
    },
    Baseline {
        name: "nth_miss_true_0",
        steps: 33,
        effects: &[("next", 12)],
    },
    Baseline {
        name: "reduce_true_0",
        steps: 32,
        effects: &[("next", 11)],
    },
    Baseline {
        name: "min_by_true_0",
        steps: 32,
        effects: &[("next", 11)],
    },
    Baseline {
        name: "max_by_true_0",
        steps: 32,
        effects: &[("next", 11)],
    },
    Baseline {
        name: "max_by_keep_true_0",
        steps: 32,
        effects: &[("next", 11)],
    },
    Baseline {
        name: "min_by_keep_true_0",
        steps: 32,
        effects: &[("next", 11)],
    },
    Baseline {
        name: "find_map_true_1",
        steps: 71,
        effects: &[("next", 11), ("callback", 36), ("next", 50)],
    },
    Baseline {
        name: "find_map_miss_true_1",
        steps: 67,
        effects: &[("next", 11), ("callback", 36), ("next", 46)],
    },
    Baseline {
        name: "position_true_1",
        steps: 71,
        effects: &[("next", 13), ("callback", 38), ("next", 50)],
    },
    Baseline {
        name: "position_miss_true_1",
        steps: 69,
        effects: &[("next", 13), ("callback", 38), ("next", 48)],
    },
    Baseline {
        name: "nth_true_1",
        steps: 65,
        effects: &[("next", 12), ("next", 44)],
    },
    Baseline {
        name: "nth_zero_true_1",
        steps: 49,
        effects: &[("next", 12)],
    },
    Baseline {
        name: "nth_miss_true_1",
        steps: 65,
        effects: &[("next", 12), ("next", 44)],
    },
    Baseline {
        name: "reduce_true_1",
        steps: 62,
        effects: &[("next", 11), ("next", 40)],
    },
    Baseline {
        name: "min_by_true_1",
        steps: 62,
        effects: &[("next", 11), ("next", 40)],
    },
    Baseline {
        name: "max_by_true_1",
        steps: 62,
        effects: &[("next", 11), ("next", 40)],
    },
    Baseline {
        name: "max_by_keep_true_1",
        steps: 62,
        effects: &[("next", 11), ("next", 40)],
    },
    Baseline {
        name: "min_by_keep_true_1",
        steps: 62,
        effects: &[("next", 11), ("next", 40)],
    },
    Baseline {
        name: "find_map_true_3",
        steps: 99,
        effects: &[
            ("next", 11),
            ("callback", 36),
            ("next", 50),
            ("callback", 75),
        ],
    },
    Baseline {
        name: "find_map_miss_true_3",
        steps: 137,
        effects: &[
            ("next", 11),
            ("callback", 36),
            ("next", 46),
            ("callback", 71),
            ("next", 81),
            ("callback", 106),
            ("next", 116),
        ],
    },
    Baseline {
        name: "position_true_3",
        steps: 93,
        effects: &[
            ("next", 13),
            ("callback", 38),
            ("next", 50),
            ("callback", 75),
        ],
    },
    Baseline {
        name: "position_miss_true_3",
        steps: 139,
        effects: &[
            ("next", 13),
            ("callback", 38),
            ("next", 48),
            ("callback", 73),
            ("next", 83),
            ("callback", 108),
            ("next", 118),
        ],
    },
    Baseline {
        name: "nth_true_3",
        steps: 81,
        effects: &[("next", 12), ("next", 44)],
    },
    Baseline {
        name: "nth_zero_true_3",
        steps: 49,
        effects: &[("next", 12)],
    },
    Baseline {
        name: "nth_miss_true_3",
        steps: 129,
        effects: &[("next", 12), ("next", 44), ("next", 76), ("next", 108)],
    },
    Baseline {
        name: "reduce_true_3",
        steps: 138,
        effects: &[
            ("next", 11),
            ("next", 40),
            ("callback", 68),
            ("next", 78),
            ("callback", 106),
            ("next", 116),
        ],
    },
    Baseline {
        name: "min_by_true_3",
        steps: 146,
        effects: &[
            ("next", 11),
            ("next", 40),
            ("callback", 68),
            ("next", 83),
            ("callback", 111),
            ("next", 124),
        ],
    },
    Baseline {
        name: "max_by_true_3",
        steps: 148,
        effects: &[
            ("next", 11),
            ("next", 40),
            ("callback", 68),
            ("next", 83),
            ("callback", 111),
            ("next", 126),
        ],
    },
    Baseline {
        name: "max_by_keep_true_3",
        steps: 132,
        effects: &[
            ("next", 11),
            ("next", 40),
            ("callback", 68),
            ("next", 75),
            ("callback", 103),
            ("next", 110),
        ],
    },
    Baseline {
        name: "min_by_keep_true_3",
        steps: 132,
        effects: &[
            ("next", 11),
            ("next", 40),
            ("callback", 68),
            ("next", 75),
            ("callback", 103),
            ("next", 110),
        ],
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
                .native_imports
                .iter()
                .filter(|import| matches!(
                    import.resolve(),
                    Some(EngineNativeOperation::Resumable(
                        EngineNativeBinding::TraitDefault(_)
                    ))
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
fn heap_decisions_preserve_aliases_and_generic_callback_results() {
    for source in [
        r#"
struct Counter<T> {val item:T,var done:bool}
impl<T> Iterator for Counter<T> {type Item=T;fn next(self)->Option<T>{if self.done {None}else{self.done=true;Some(self.item)}}}
fn transform<I:Iterator<Item=ArrayList<i32>>>(source:I)->Option<ArrayList<i32>> {
    source.find_map(|n|{val fallback:Option<ArrayList<i32>> = None;Some(fallback.unwrap_or_else(||[n[0]+n[1]]))})
}
fn main()->i32 {val source=Counter{item:[20,22],done:false};transform(source).unwrap_or([0])[0]}
"#,
        r#"
fn main()->i32 {
    val values=[[20],[22]];
    val reduced=values.iter().reduce(|a,n|{val fallback:Option<ArrayList<i32>> = None;fallback.unwrap_or_else(||[a[0]+n[0]])});
    values.push([1]);reduced.unwrap_or([0])[0]
}
"#,
        r#"
fn main()->i32 {
    val first=[20];val second=[22];val values=[first,second];var visits=0;
    val selected=values.iter().min_by(|a,n|{visits+=1;a.push(1);Ordering::Equal}).unwrap_or([0]);
    selected.push(2);std::debug::assert_eq(first.len(),3,"first tie");
    val last=values.iter().max_by(|a,n|{visits+=1;Ordering::Equal}).unwrap_or([0]);
    last.push(3);std::debug::assert_eq(second.len(),2,"last tie");
    std::debug::assert_eq(visits,2,"once");values.push([0]);selected[0]+last[0]
}
"#,
        r#"
struct Single<T> {val item:T,var done:bool}
impl<T> Iterator for Single<T> {
    type Item=T;
    fn reduce(self,combine:fn(T,T)->T)->Option<T> {Some(self.item)}
    fn next(self)->Option<T> {if self.done {None}else{self.done=true;Some(self.item)}}
}
fn main()->i32 {val source=Single{item:[40,2],done:false};val result=source.reduce(|a,n|{val bad=2147483647;[bad+1]});source.nth(0).unwrap_or([0])[0]+result.unwrap_or([0,0])[1]}
"#,
    ] {
        let program = compile_test_bytecode(source);
        for encoded in [false, true] {
            let mut runtime = runtime();
            let loaded = runtime
                .load_program("decisions", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(runtime);
            assert_eq!(
                vm.execute(&loaded, "main").unwrap().return_value,
                Value::I32(42)
            );
            assert_clean(&vm);
            assert!(vm.runtime().gc().stats().collections > 0);
        }
    }
}

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

fn assert_clean(vm: &Vm) {
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert!(!vm.runtime().is_quarantined());
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.runtime().gc().allocated_objects(), 0);
}

#[test]
fn decision_callbacks_preserve_trap_frames_and_session_cancellation() {
    for call in [
        "source.find_map(|n|{val x=2147483647;Some(x+1)})",
        "source.position(|n|{val x=2147483647;x+1==n})",
        "source.reduce(|a,n|{val x=2147483647;x+1})",
        "source.min_by(|a,n|{val x=2147483647;val y=x+1;Ordering::Equal})",
        "source.max_by(|a,n|{val x=2147483647;val y=x+1;Ordering::Equal})",
    ] {
        let program = compile_test_bytecode(&format!(
            "fn main()->i32 {{val source=[1,2].iter();val out={call};0}} fn ready()->i32{{7}}"
        ));
        for encoded in [false, true] {
            let mut runtime = runtime();
            let loaded = runtime
                .load_program("trap", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(runtime);
            let Err(VmError::RuntimeError(error)) = vm.execute(&loaded, "main") else {
                panic!("expected callback trap")
            };
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
            let trace = error.trace().unwrap();
            assert_eq!(trace.frames.len(), 2);
            assert!(trace.frames[0].function_name.contains("closure"));
            assert_eq!(trace.frames[1].function_name, "main");
            assert_clean(&vm);
            assert_eq!(
                vm.execute(&loaded, "ready").unwrap().return_value,
                Value::I32(7)
            );
        }
    }
    let program = compile_test_bytecode(
        "fn main()->i32 {val values=[[20],[22]];values.iter().reduce(|a,n|{print(\"cancel\");[a[0]+n[0]]}).unwrap_or([0])[0]} fn ready()->i32 {7}",
    );
    for encoded in [false, true] {
        let token = CancellationToken::default();
        let cancel = token.clone();
        let effects = Rc::new(Cell::new(0));
        let sink = effects.clone();
        let mut runtime = runtime();
        runtime
            .register_host_function(HostFunction::new(standard_log(), move |_, _| {
                sink.set(sink.get() + 1);
                cancel.cancel();
                Ok(Value::Unit)
            }))
            .unwrap();
        let loaded = runtime
            .load_program("cancel", route(&program, encoded))
            .unwrap();
        let mut options = runtime.execution_options();
        options.cancellation = token;
        let session = runtime.begin_execution(&loaded, options).unwrap();
        let mut vm = Vm::new(runtime);
        assert!(
            matches!(vm.execute(&loaded,"main"),Err(VmError::RuntimeError(error)) if error.kind()==RuntimeErrorKind::Cancelled)
        );
        assert_eq!(effects.get(), 1);
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
        assert!(vm.execute(&loaded, "ready").is_err());
        drop(session);
        assert_clean(&vm);
        assert_eq!(
            vm.execute(&loaded, "ready").unwrap().return_value,
            Value::I32(7)
        );
    }
}

#[test]
fn host_reentry_preserves_pending_reduction_and_selected_next() {
    let program = compile_test_bytecode(
        r#"
struct Counter {var n:i32}
impl Iterator for Counter {type Item=ArrayList<i32>;fn next(self)->Option<ArrayList<i32>> {print("next");if self.n==0 {None}else{self.n-=1;Some([21])}}}
fn main()->i32 {Counter{n:2}.reduce(|a,n|{print("reduce");[a[0]+n[0]]}).unwrap_or([0])[0]}
fn inner()->i32 {[7].iter().nth(0).unwrap_or(0)}
fn fail()->i32 {[1,2].iter().reduce(|a,n|{val x=2147483647;x+1}).unwrap_or(0)}
"#,
    );
    let root = &program.modules[program.root.index()];
    let inner = root
        .functions
        .iter()
        .find(|function| function.name == "inner")
        .unwrap()
        .id;
    let fail = root
        .functions
        .iter()
        .find(|function| function.name == "fail")
        .unwrap()
        .id;
    for encoded in [false, true] {
        let effects = Rc::new(RefCell::new(Vec::new()));
        let sink = effects.clone();
        let mut runtime = runtime();
        runtime
            .register_host_function(HostFunction::new(standard_log(), move |context, args| {
                sink.borrow_mut().push(args[0].clone());
                let root = context.runtime().execution_root().unwrap();
                assert_eq!(
                    reenter(context, &root, inner, &[]).unwrap().value(),
                    Value::I32(7)
                );
                assert!(reenter(context, &root, fail, &[]).is_err());
                assert_eq!(
                    context.runtime().resources().counters().current_call_depth,
                    2
                );
                context.runtime().collect_garbage().unwrap();
                Ok(Value::Unit)
            }))
            .unwrap();
        let loaded = runtime
            .load_program("reentry", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        assert_eq!(
            *effects.borrow(),
            [
                Value::Str("next".into()),
                Value::Str("next".into()),
                Value::Str("reduce".into()),
                Value::Str("next".into())
            ]
        );
        assert_clean(&vm);
    }
}
