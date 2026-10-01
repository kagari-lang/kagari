use crate::{Vm, VmError, tests::common::compile_test_bytecode};
use kagari_abi::{callable::EngineNativeBinding, native_import::EngineNativeOperation};
use kagari_bytecode::{BytecodeProgram, KbcArtifact};
use kagari_common::{
    cancellation::CancellationToken,
    host_interface::standard_log,
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
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
    for custom_iter in [false, true] {
        for custom_ord in [false, true] {
            for length in [0, 1, 5] {
                for method in ["min", "max", "min_by_key", "max_by_key"] {
                    let keyed = method.ends_with("key");
                    let fields = match length {
                        0 => vec![],
                        1 => vec![(2, 20)],
                        _ => vec![(2, 20), (1, 10), (1, 11), (3, 30), (3, 31)],
                    };
                    let item_ty = if keyed {
                        "ArrayList<i32>"
                    } else if custom_ord {
                        "Rank<i32>"
                    } else {
                        "i32"
                    };
                    let items = fields
                        .iter()
                        .map(|(value, tag)| {
                            if keyed {
                                format!("[{value},{tag}]")
                            } else if custom_ord {
                                format!("Rank{{value:{value},tag:{tag}}}")
                            } else {
                                value.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    let source = if custom_iter {
                        "Counter{items:values,index:0}"
                    } else {
                        "values.iter()"
                    };
                    let call = if keyed {
                        format!(
                            "source.{method}(|n|{{print(\"key\");{}}})",
                            if custom_ord {
                                "Rank{value:n[0],tag:n[1]}"
                            } else {
                                "n[0]"
                            }
                        )
                    } else {
                        format!("source.{method}()")
                    };
                    let selected = if length == 0 {
                        None
                    } else if length == 1 {
                        Some((2, 20))
                    } else if method.starts_with("min") {
                        Some((1, 10))
                    } else {
                        Some((3, 31))
                    };
                    let check = if let Some((value, tag)) = selected {
                        if keyed {
                            format!(
                                "std::debug::assert_eq(result.unwrap_or([0,0])[1],{tag},\"selected\");"
                            )
                        } else if custom_ord {
                            format!(
                                "std::debug::assert_eq(result.unwrap_or(Rank{{value:0,tag:0}}).tag,{tag},\"selected\");"
                            )
                        } else {
                            format!("std::debug::assert_eq(result,Some({value}),\"selected\");")
                        }
                    } else {
                        "std::debug::assert(result.is_none(),\"empty\");".into()
                    };
                    let name = format!("{method}_{custom_iter}_{custom_ord}_{length}");
                    let source = format!(
                        r#"
struct Counter<T> {{val items:ArrayList<T>,var index:usize}}
impl<T> Iterator for Counter<T> {{type Item=T;fn next(self)->Option<T>{{print("next");if self.index>=self.items.len() {{None}}else{{val item=self.items[self.index];self.index+=1;Some(item)}}}}}}
struct Rank<T> {{val value:T,val tag:i32}}
impl<T:PartialEq> PartialEq for Rank<T> {{fn eq(self,other:Self)->bool {{self.value==other.value}}}}
impl<T:Eq> Eq for Rank<T> {{}}
impl<T:PartialOrd> PartialOrd for Rank<T> {{fn partial_cmp(self,other:Self)->Option<Ordering> {{self.value.partial_cmp(other.value)}}}}
impl<T:Ord> Ord for Rank<T> {{fn cmp(self,other:Self)->Ordering {{print("cmp");self.value.cmp(other.value)}}}}
fn consume<I:Iterator<Item={item_ty}>>(source:I)->Option<{item_ty}> {{{call}}}
fn main()->i32 {{val values:ArrayList<{item_ty}> = [{items}];val result=consume({source});{check}42}}
"#
                    );
                    // Keep comparison tokens; only separate a generic close from assignment.
                    let source = source.replace("self.index> =", "self.index>=");
                    cases.push((name, source));
                }
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
// Observed at d3bfe82 before replacing these compiler extrema algorithms.
const BASELINE: &[Baseline] = &[
    Baseline {
        name: "min_false_false_0",
        steps: 24,
        effects: &[],
    },
    Baseline {
        name: "max_false_false_0",
        steps: 24,
        effects: &[],
    },
    Baseline {
        name: "min_by_key_false_false_0",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "max_by_key_false_false_0",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "min_false_false_1",
        steps: 35,
        effects: &[],
    },
    Baseline {
        name: "max_false_false_1",
        steps: 35,
        effects: &[],
    },
    Baseline {
        name: "min_by_key_false_false_1",
        steps: 53,
        effects: &[("key", 23)],
    },
    Baseline {
        name: "max_by_key_false_false_1",
        steps: 53,
        effects: &[("key", 23)],
    },
    Baseline {
        name: "min_false_false_5",
        steps: 81,
        effects: &[],
    },
    Baseline {
        name: "max_false_false_5",
        steps: 83,
        effects: &[],
    },
    Baseline {
        name: "min_by_key_false_false_5",
        steps: 141,
        effects: &[
            ("key", 35),
            ("key", 52),
            ("key", 74),
            ("key", 92),
            ("key", 110),
        ],
    },
    Baseline {
        name: "max_by_key_false_false_5",
        steps: 145,
        effects: &[
            ("key", 35),
            ("key", 52),
            ("key", 70),
            ("key", 88),
            ("key", 110),
        ],
    },
    Baseline {
        name: "min_false_true_0",
        steps: 24,
        effects: &[],
    },
    Baseline {
        name: "max_false_true_0",
        steps: 24,
        effects: &[],
    },
    Baseline {
        name: "min_by_key_false_true_0",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "max_by_key_false_true_0",
        steps: 26,
        effects: &[],
    },
    Baseline {
        name: "min_false_true_1",
        steps: 41,
        effects: &[],
    },
    Baseline {
        name: "max_false_true_1",
        steps: 41,
        effects: &[],
    },
    Baseline {
        name: "min_by_key_false_true_1",
        steps: 57,
        effects: &[("key", 23)],
    },
    Baseline {
        name: "max_by_key_false_true_1",
        steps: 57,
        effects: &[("key", 23)],
    },
    Baseline {
        name: "min_false_true_5",
        steps: 127,
        effects: &[("cmp", 44), ("cmp", 64), ("cmp", 82), ("cmp", 100)],
    },
    Baseline {
        name: "max_false_true_5",
        steps: 129,
        effects: &[("cmp", 44), ("cmp", 62), ("cmp", 80), ("cmp", 100)],
    },
    Baseline {
        name: "min_by_key_false_true_5",
        steps: 193,
        effects: &[
            ("key", 35),
            ("key", 56),
            ("cmp", 71),
            ("key", 90),
            ("cmp", 105),
            ("key", 120),
            ("cmp", 135),
            ("key", 150),
            ("cmp", 165),
        ],
    },
    Baseline {
        name: "max_by_key_false_true_5",
        steps: 197,
        effects: &[
            ("key", 35),
            ("key", 56),
            ("cmp", 71),
            ("key", 86),
            ("cmp", 101),
            ("key", 116),
            ("cmp", 131),
            ("key", 150),
            ("cmp", 165),
        ],
    },
    Baseline {
        name: "min_true_false_0",
        steps: 33,
        effects: &[("next", 12)],
    },
    Baseline {
        name: "max_true_false_0",
        steps: 33,
        effects: &[("next", 12)],
    },
    Baseline {
        name: "min_by_key_true_false_0",
        steps: 35,
        effects: &[("next", 14)],
    },
    Baseline {
        name: "max_by_key_true_false_0",
        steps: 35,
        effects: &[("next", 14)],
    },
    Baseline {
        name: "min_true_false_1",
        steps: 69,
        effects: &[("next", 13), ("next", 46)],
    },
    Baseline {
        name: "max_true_false_1",
        steps: 69,
        effects: &[("next", 13), ("next", 46)],
    },
    Baseline {
        name: "min_by_key_true_false_1",
        steps: 87,
        effects: &[("next", 17), ("key", 46), ("next", 59)],
    },
    Baseline {
        name: "max_by_key_true_false_1",
        steps: 87,
        effects: &[("next", 17), ("key", 46), ("next", 59)],
    },
    Baseline {
        name: "min_true_false_5",
        steps: 215,
        effects: &[
            ("next", 17),
            ("next", 50),
            ("next", 87),
            ("next", 122),
            ("next", 157),
            ("next", 192),
        ],
    },
    Baseline {
        name: "max_true_false_5",
        steps: 217,
        effects: &[
            ("next", 17),
            ("next", 50),
            ("next", 85),
            ("next", 120),
            ("next", 157),
            ("next", 194),
        ],
    },
    Baseline {
        name: "min_by_key_true_false_5",
        steps: 275,
        effects: &[
            ("next", 29),
            ("key", 58),
            ("next", 71),
            ("key", 100),
            ("next", 118),
            ("key", 147),
            ("next", 161),
            ("key", 190),
            ("next", 204),
            ("key", 233),
            ("next", 247),
        ],
    },
    Baseline {
        name: "max_by_key_true_false_5",
        steps: 279,
        effects: &[
            ("next", 29),
            ("key", 58),
            ("next", 71),
            ("key", 100),
            ("next", 114),
            ("key", 143),
            ("next", 157),
            ("key", 186),
            ("next", 204),
            ("key", 233),
            ("next", 251),
        ],
    },
    Baseline {
        name: "min_true_true_0",
        steps: 33,
        effects: &[("next", 12)],
    },
    Baseline {
        name: "max_true_true_0",
        steps: 33,
        effects: &[("next", 12)],
    },
    Baseline {
        name: "min_by_key_true_true_0",
        steps: 35,
        effects: &[("next", 14)],
    },
    Baseline {
        name: "max_by_key_true_true_0",
        steps: 35,
        effects: &[("next", 14)],
    },
    Baseline {
        name: "min_true_true_1",
        steps: 75,
        effects: &[("next", 15), ("next", 48)],
    },
    Baseline {
        name: "max_true_true_1",
        steps: 75,
        effects: &[("next", 15), ("next", 48)],
    },
    Baseline {
        name: "min_by_key_true_true_1",
        steps: 91,
        effects: &[("next", 17), ("key", 46), ("next", 63)],
    },
    Baseline {
        name: "max_by_key_true_true_1",
        steps: 91,
        effects: &[("next", 17), ("key", 46), ("next", 63)],
    },
    Baseline {
        name: "min_true_true_5",
        steps: 261,
        effects: &[
            ("next", 27),
            ("next", 60),
            ("cmp", 92),
            ("next", 105),
            ("cmp", 137),
            ("next", 148),
            ("cmp", 180),
            ("next", 191),
            ("cmp", 223),
            ("next", 234),
        ],
    },
    Baseline {
        name: "max_true_true_5",
        steps: 263,
        effects: &[
            ("next", 27),
            ("next", 60),
            ("cmp", 92),
            ("next", 103),
            ("cmp", 135),
            ("next", 146),
            ("cmp", 178),
            ("next", 191),
            ("cmp", 223),
            ("next", 236),
        ],
    },
    Baseline {
        name: "min_by_key_true_true_5",
        steps: 327,
        effects: &[
            ("next", 29),
            ("key", 58),
            ("next", 75),
            ("key", 104),
            ("cmp", 119),
            ("next", 134),
            ("key", 163),
            ("cmp", 178),
            ("next", 189),
            ("key", 218),
            ("cmp", 233),
            ("next", 244),
            ("key", 273),
            ("cmp", 288),
            ("next", 299),
        ],
    },
    Baseline {
        name: "max_by_key_true_true_5",
        steps: 331,
        effects: &[
            ("next", 29),
            ("key", 58),
            ("next", 75),
            ("key", 104),
            ("cmp", 119),
            ("next", 130),
            ("key", 159),
            ("cmp", 174),
            ("next", 185),
            ("key", 214),
            ("cmp", 229),
            ("next", 244),
            ("key", 273),
            ("cmp", 288),
            ("next", 303),
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
fn extrema_root_heap_keys_and_preserve_builtin_ordering_and_aliases() {
    for source in [
        r#"
struct Key {val payload:ArrayList<i32>}
impl PartialEq for Key {fn eq(self,other:Self)->bool {self.payload[0]==other.payload[0]}}
impl Eq for Key {}
impl PartialOrd for Key {fn partial_cmp(self,other:Self)->Option<Ordering> {self.payload[0].partial_cmp(other.payload[0])}}
impl Ord for Key {fn cmp(self,other:Self)->Ordering {
    val missing:Option<ArrayList<i32>> = None;
    val kept=missing.unwrap_or_else(||[self.payload[0],other.payload[0]]);
    kept[0].cmp(kept[1])
}}
fn smallest<I:Iterator<Item=ArrayList<i32>>>(source:I,key:fn(ArrayList<i32>)->Key)->Option<ArrayList<i32>> {source.min_by_key(key)}
fn main()->i32 {
    val a=[20];val b=[22];val values=[a,b];var calls=0;
    val key=|n:ArrayList<i32>|{calls+=1;Key{payload:[1,n[0]]}};
    val first=smallest(values.iter(),key).unwrap_or([0]);
    val last=values.iter().max_by_key(key).unwrap_or([0]);
    first.push(1);last.push(2);std::debug::assert_eq(a.len(),2,"first alias");
    std::debug::assert_eq(b.len(),2,"last alias");std::debug::assert_eq(calls,4,"once per item");
    values.push([0]);first[0]+last[0]
}
"#,
        r#"
fn main()->i32 {
    std::debug::assert_eq(["z","a","a"].iter().min(),Some("a"),"string min");
    std::debug::assert_eq(["z","a"].iter().max(),Some("z"),"string max");
    std::debug::assert_eq([Ordering::Greater,Ordering::Equal,Ordering::Less].iter().min(),Some(Ordering::Less),"ordering min");
    std::debug::assert_eq([Ordering::Greater,Ordering::Equal].iter().max(),Some(Ordering::Greater),"ordering max");42
}
"#,
    ] {
        let program = compile_test_bytecode(source);
        for encoded in [false, true] {
            let mut runtime = runtime();
            let loaded = runtime
                .load_program("heap-keys", route(&program, encoded))
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
fn ordering_and_key_traps_preserve_origins_and_release_iteration_guards() {
    for call in [
        "source.min()",
        "source.max()",
        "source.min_by_key(|n|n)",
        "source.max_by_key(|n|n)",
        "[1,2].iter().min_by_key(|n|{val x=2147483647;Rank{value:x+1}})",
        "[1,2].iter().max_by_key(|n|{val x=2147483647;Rank{value:x+1}})",
    ] {
        let program = compile_test_bytecode(&format!(
            r#"
struct Rank {{val value:i32}}
impl PartialEq for Rank {{fn eq(self,other:Self)->bool {{self.value==other.value}}}}
impl Eq for Rank {{}}
impl PartialOrd for Rank {{fn partial_cmp(self,other:Self)->Option<Ordering> {{self.value.partial_cmp(other.value)}}}}
impl Ord for Rank {{fn cmp(self,other:Self)->Ordering {{val x=2147483647;val fail=x+1;Ordering::Equal}}}}
fn main()->i32 {{val source=[Rank{{value:1}},Rank{{value:2}}].iter();val out={call};0}}
fn ready()->i32 {{7}}
"#
        ));
        for encoded in [false, true] {
            let mut runtime = runtime();
            let loaded = runtime
                .load_program("cmp-trap", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(runtime);
            let Err(VmError::RuntimeError(error)) = vm.execute(&loaded, "main") else {
                panic!("expected trap")
            };
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
            let trace = error.trace().unwrap();
            assert_eq!(trace.frames.len(), 2);
            assert!(
                trace.frames[0].function_name.contains("cmp")
                    || trace.frames[0].function_name.contains("closure")
            );
            assert_eq!(trace.frames[1].function_name, "main");
            assert_clean(&vm);
            assert_eq!(
                vm.execute(&loaded, "ready").unwrap().return_value,
                Value::I32(7)
            );
        }
    }
}

#[test]
fn comparison_reentry_and_cancellation_preserve_the_pending_key_state() {
    let program = compile_test_bytecode(
        r#"
struct Key {val value:i32}
impl PartialEq for Key {fn eq(self,other:Self)->bool {self.value==other.value}}
impl Eq for Key {}
impl PartialOrd for Key {fn partial_cmp(self,other:Self)->Option<Ordering> {self.value.partial_cmp(other.value)}}
impl Ord for Key {fn cmp(self,other:Self)->Ordering {print("cmp");self.value.cmp(other.value)}}
fn main()->i32 {[20,22].iter().max_by_key(|n|{print("key");Key{value:1}}).unwrap_or(0)+20}
fn inner()->i32 {[7,1].iter().max().unwrap_or(0)}
fn fail()->i32 {[1].iter().min_by_key(|n|{val x=2147483647;x+1}).unwrap_or(0)}
fn ready()->i32 {7}
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
        for cancellation in [false, true] {
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            let token = CancellationToken::default();
            let cancel = token.clone();
            let mut runtime = runtime();
            runtime
                .register_host_function(HostFunction::new(standard_log(), move |context, args| {
                    sink.borrow_mut().push(args[0].clone());
                    if cancellation {
                        if args == [Value::Str("cmp".into())] {
                            cancel.cancel();
                        }
                    } else {
                        let root = context.runtime().execution_root().unwrap();
                        assert_eq!(
                            crate::reenter(context, &root, inner, &[]).unwrap().value(),
                            Value::I32(7)
                        );
                        assert!(crate::reenter(context, &root, fail, &[]).is_err());
                        assert_eq!(
                            context.runtime().resources().counters().current_call_depth,
                            2
                        );
                    }
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::Unit)
                }))
                .unwrap();
            let loaded = runtime
                .load_program("cmp-reentry", route(&program, encoded))
                .unwrap();
            let mut options = runtime.execution_options();
            options.cancellation = token;
            let session = runtime.begin_execution(&loaded, options).unwrap();
            let mut vm = Vm::new(runtime);
            let result = vm.execute(&loaded, "main");
            if cancellation {
                assert!(
                    matches!(result,Err(VmError::RuntimeError(error)) if error.kind()==RuntimeErrorKind::Cancelled)
                );
                assert!(vm.execute(&loaded, "ready").is_err());
            } else {
                assert_eq!(result.unwrap().return_value, Value::I32(42));
            }
            assert_eq!(
                *effects.borrow(),
                [
                    Value::Str("key".into()),
                    Value::Str("key".into()),
                    Value::Str("cmp".into())
                ]
            );
            assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
            drop(session);
            assert_clean(&vm);
            assert_eq!(
                vm.execute(&loaded, "ready").unwrap().return_value,
                Value::I32(7)
            );
        }
    }
}

#[test]
fn imported_generic_ordering_materializes_the_selected_dependency_target() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub struct Rank<T> {pub val value:T}
impl<T:PartialEq> PartialEq for Rank<T> {fn eq(self,other:Self)->bool {self.value==other.value}}
impl<T:Eq> Eq for Rank<T> {}
impl<T:PartialOrd> PartialOrd for Rank<T> {fn partial_cmp(self,other:Self)->Option<Ordering> {self.value.partial_cmp(other.value)}}
impl<T:Ord> Ord for Rank<T> {fn cmp(self,other:Self)->Ordering {self.value.cmp(other.value)}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::Rank;
fn largest<I:Iterator<Item=Rank<i32>>>(source:I)->Option<Rank<i32>> {source.max()}
fn main()->i32 {
    val items=[Rank{value:20},Rank{value:22}];
    val first=items.iter().min().unwrap_or(Rank{value:0});
    val last=largest(items.iter()).unwrap_or(Rank{value:0});first.value+last.value
}
"#,
        ),
    ] {
        let uri = format!("mem://{name}");
        sources
            .bind_module(
                &uri,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        root = Some(sources.set(&uri, text.into(), SourceLayer::Base).unwrap());
    }
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let checked = snapshot
        .check_program(root.unwrap(), &Default::default())
        .unwrap();
    let ir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let program = lower_program_to_bytecode(&ir).unwrap();
    for encoded in [false, true] {
        let mut runtime = runtime();
        let loaded = runtime
            .load_program("foreign-cmp", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        assert_clean(&vm);
    }
}
