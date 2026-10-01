use crate::{error::VmError, reentry::reenter, tests::common::compile_test_bytecode, vm::Vm};

use kagari_abi::{
    callable::EngineNativeBinding, native_import::EngineNativeOperation,
    standard::bindings::NativeDefaultMethod,
};
use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::{
    cancellation::CancellationToken,
    host_interface::standard_log,
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
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
    for kind in ["native", "custom", "lazy", "list", "dynamic"] {
        for length in [0, 1, 3] {
            let items = match length {
                0 => "",
                1 => "\"中文😀\"",
                _ => "\"中文😀\",\"\",\"c\"",
            };
            let source = match kind {
                "native" => "consume(values.iter(), separator())",
                "custom" => "consume(Counter{items:values,index:0}, separator())",
                "lazy" => "consume(values.iter().map(|s|{print(\"item\");s}), separator())",
                "list" => "Sequence{items:values}.join(separator())",
                _ => "{val view:List<String> = Sequence{items:values};view.join(separator())}",
            };
            let expected = match length {
                0 => "",
                1 => "中文😀",
                _ => "中文😀//c",
            };
            cases.push((format!("{kind}_{length}"), format!(r#"
struct Counter<T> {{val items:ArrayList<T>,var index:usize}}
impl<T> Iterator for Counter<T> {{type Item=T;fn next(self)->Option<T>{{print("next");if self.index>=self.items.len(){{None}}else{{val item=self.items[self.index];self.index+=1;Some(item)}}}}}}
struct Sequence {{val items:ArrayList<String>}}
impl Index<usize> for Sequence {{type Output=String;fn index(self,index:usize)->String{{self.items[index]}}}}
impl Iterable for Sequence {{type Item=String;type Iter=Iter<String>;fn iter(self)->Iter<String>{{print("iter");self.items.iter()}}}}
impl List<String> for Sequence {{fn len(self)->usize{{self.items.len()}}fn is_empty(self)->bool{{self.items.is_empty()}}fn get(self,index:usize)->Option<String>{{self.items.get(index)}}}}
fn separator()->String {{print("separator");"/"}}
fn consume<I:Iterator<Item=String>>(source:I,separator:String)->String {{source.join(separator)}}
fn main()->i32 {{val values:ArrayList<String> = [{items}];val text={source};std::debug::assert_eq(text,"{expected}","joined");print("done");42}}
"#)));
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
// Observed at b9a33ed before replacing compiler-owned join traversal.
const BASELINE: &[Baseline] = &[
    Baseline {
        name: "native_0",
        steps: 34,
        effects: &[("separator", 7), ("done", 32)],
    },
    Baseline {
        name: "native_1",
        steps: 41,
        effects: &[("separator", 8), ("done", 39)],
    },
    Baseline {
        name: "native_3",
        steps: 55,
        effects: &[("separator", 10), ("done", 53)],
    },
    Baseline {
        name: "custom_0",
        steps: 43,
        effects: &[("separator", 8), ("next", 18), ("done", 41)],
    },
    Baseline {
        name: "custom_1",
        steps: 75,
        effects: &[("separator", 9), ("next", 19), ("next", 50), ("done", 73)],
    },
    Baseline {
        name: "custom_3",
        steps: 139,
        effects: &[
            ("separator", 11),
            ("next", 21),
            ("next", 52),
            ("next", 83),
            ("next", 114),
            ("done", 137),
        ],
    },
    Baseline {
        name: "lazy_0",
        steps: 48,
        effects: &[("separator", 11), ("done", 46)],
    },
    Baseline {
        name: "lazy_1",
        steps: 69,
        effects: &[("separator", 12), ("item", 34), ("done", 67)],
    },
    Baseline {
        name: "lazy_3",
        steps: 111,
        effects: &[
            ("separator", 14),
            ("item", 36),
            ("item", 56),
            ("item", 76),
            ("done", 109),
        ],
    },
    Baseline {
        name: "list_0",
        steps: 37,
        effects: &[("separator", 7), ("iter", 12), ("done", 35)],
    },
    Baseline {
        name: "list_1",
        steps: 44,
        effects: &[("separator", 8), ("iter", 13), ("done", 42)],
    },
    Baseline {
        name: "list_3",
        steps: 58,
        effects: &[("separator", 10), ("iter", 15), ("done", 56)],
    },
    Baseline {
        name: "dynamic_0",
        steps: 40,
        effects: &[("separator", 10), ("iter", 15), ("done", 38)],
    },
    Baseline {
        name: "dynamic_1",
        steps: 47,
        effects: &[("separator", 11), ("iter", 16), ("done", 45)],
    },
    Baseline {
        name: "dynamic_3",
        steps: 61,
        effects: &[("separator", 13), ("iter", 18), ("done", 59)],
    },
];
#[test]
fn join_preserves_effects_and_every_budget_cut() {
    let cases = cases();
    assert_eq!(cases.len(), BASELINE.len());
    for (name, source) in cases {
        let baseline = BASELINE.iter().find(|case| case.name == name).unwrap();
        let program = compile_test_bytecode(&source);
        let defaults: Vec<_> = program.modules[program.root.index()]
            .native_imports
            .iter()
            .filter_map(|import| match import.resolve() {
                Some(EngineNativeOperation::Resumable(EngineNativeBinding::TraitDefault(
                    operation,
                ))) => Some(operation),
                _ => None,
            })
            .collect();
        let lazy = name.starts_with("lazy_");
        let join = if name.starts_with("list_") || name.starts_with("dynamic_") {
            NativeDefaultMethod::ListJoin
        } else {
            NativeDefaultMethod::Join
        };
        assert_eq!(
            defaults
                .iter()
                .filter(|operation| **operation == join)
                .count(),
            1,
            "{name} join entry"
        );
        assert_eq!(
            defaults
                .iter()
                .filter(|operation| **operation == NativeDefaultMethod::Map)
                .count(),
            usize::from(lazy),
            "{name} lazy entry"
        );
        assert_eq!(
            defaults.len(),
            1 + usize::from(lazy),
            "{name} native defaults"
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
            let loaded = runtime.load_program("join-native", program).unwrap();
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
fn join_stops_at_first_none_and_preserves_iterator_progress() {
    let program = compile_test_bytecode(
        r#"
struct Sometimes {var calls:i32}
impl Iterator for Sometimes {type Item=String;fn next(self)->Option<String>{self.calls+=1;if self.calls==2 {None}else{Some(f"{self.calls}")}}}
fn main()->i32 {
    val source=Sometimes{calls:0};
    std::debug::assert(source.join("/")=="1" && source.calls==2,"first None");
    std::debug::assert(source.next()==Some("3"),"remaining progress");
    val storage=["a","b","c"];val cursor=storage.iter();cursor.next();
    std::debug::assert(cursor.join("/")=="b/c","remaining items");
    storage.push("d");42
}
"#,
    );
    for encoded in [false, true] {
        let mut runtime = runtime();
        let loaded = runtime
            .load_program("join-progress", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        assert_clean(&vm);
    }
}

#[test]
fn join_reentry_and_cancellation_keep_accumulated_items_rooted() {
    let program = compile_test_bytecode(
        r#"
fn main()->i32 {
    val text=["中文😀","b"].iter().map(|s|{print("item");s}).join("/");
    std::debug::assert_eq(text,"中文😀/b","accumulator");42
}
fn inner()->String {["x","y"].iter().join("-")}
fn fail()->String {[1].iter().map(|n|f"{n/0}").join("-")}
fn ready()->i32 {7}
"#,
    );
    let root = &program.modules[program.root.index()];
    let inner = root
        .functions
        .iter()
        .find(|f| f.name == "inner")
        .unwrap()
        .id;
    let fail = root.functions.iter().find(|f| f.name == "fail").unwrap().id;
    for encoded in [false, true] {
        for cancellation in [false, true] {
            let calls = Rc::new(Cell::new(0));
            let count = calls.clone();
            let token = CancellationToken::default();
            let cancel = token.clone();
            let mut runtime = runtime();
            runtime
                .register_host_function(HostFunction::new(standard_log(), move |context, args| {
                    assert_eq!(args, [Value::Str("item".into())]);
                    count.set(count.get() + 1);
                    if cancellation {
                        if count.get() == 2 {
                            cancel.cancel();
                        }
                    } else {
                        let root = context.runtime().execution_root().unwrap();
                        assert_eq!(
                            reenter(context, &root, inner, &[]).unwrap().value(),
                            Value::Str("x-y".into())
                        );
                        assert!(reenter(context, &root, fail, &[]).is_err());
                        // The lazy step function and map callback share the
                        // caller's ordinary frame stack.
                        assert_eq!(
                            context.runtime().resources().counters().current_call_depth,
                            3
                        );
                    }
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::Unit)
                }))
                .unwrap();
            let loaded = runtime
                .load_program("join-reentry", route(&program, encoded))
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
            assert_eq!(calls.get(), 2);
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
fn join_materializes_imported_generic_iterator_targets() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub struct Counter<T> {pub val items:ArrayList<T>,pub var index:usize}
impl<T> Iterator for Counter<T> {type Item=T;fn next(self)->Option<T>{if self.index>=self.items.len(){None}else{val item=self.items[self.index];self.index+=1;Some(item)}}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::Counter;
fn consume<I:Iterator<Item=String>>(source:I)->String {source.join("/")}
fn main()->i32 {val text=consume(Counter{items:["a","b"],index:0});std::debug::assert_eq(text,"a/b","foreign iterator");42}
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
            .load_program("foreign-join", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        assert_clean(&vm);
    }
}
