use crate::{Vm, VmError, tests::common::compile_test_bytecode};
use kagari_abi::{callable::EngineNativeBinding, standard::bindings::NativeDefaultMethod};
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

mod baseline;
mod cases;
mod numeric;
mod numeric_baseline;
mod numeric_boundaries;
mod numeric_cases;
use baseline::BASELINE;
use cases::cases;

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

#[test]
fn aggregates_preserve_effects_and_every_budget_cut() {
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
                    import.binding,
                    EngineNativeBinding::TraitDefault(
                        NativeDefaultMethod::Sum | NativeDefaultMethod::Product
                    )
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
            let loaded = runtime.load_program("aggregate-native", program).unwrap();
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
fn generic_destination_methods_and_next_targets_materialize_in_their_owner() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub struct Counter<T> {pub val items:ArrayList<T>,pub var index:usize}
impl<T> Iterator for Counter<T> {type Item=T;fn next(self)->Option<T>{if self.index>=self.items.len(){None}else{val item=self.items[self.index];self.index+=1;Some(item)}}}
pub struct Bucket<T> {pub val items:ArrayList<T>}
impl<T> Sum<T> for Bucket<T> {fn sum<I:Iterable<Item=T>>(source:I)->Self {val items:ArrayList<T> = ArrayList::new();for item in source {items.push(item);}Bucket{items}}}
impl<T> Product<T> for Bucket<T> {fn product<I:Iterable<Item=T>>(source:I)->Self {val items:ArrayList<T> = ArrayList::new();for item in source {items.push(item);}Bucket{items}}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Counter,Bucket};
fn total<I:Iterator<Item=i32>>(source:I)->Bucket<i32> {source.sum()}
fn multiply<I:Iterator<Item=i32>>(source:I)->Bucket<i32> {source.product()}
fn main()->i32 {
    val sum=total(Counter{items:[20,22],index:0});
    val product=multiply(Counter{items:[20,22],index:0});
    std::debug::assert_eq(sum.items.len(),2usize,"sum destination");
    std::debug::assert_eq(product.items.len(),2usize,"product destination");
    std::debug::assert_eq(sum.items[0]+product.items[1],42,"preserved items");42
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
            .load_program("foreign-aggregation", route(&program, encoded))
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
fn user_aggregation_reentry_and_cancellation_preserve_the_forwarded_call() {
    let program = compile_test_bytecode(
        r#"
struct Total {val value:i32}
impl Sum<i32> for Total {fn sum<I:Iterable<Item=i32>>(source:I)->Self {print("sum");val value=source.iter().fold(0,|a,n|{print("combine");a+n});Total{value}}}
impl Product<i32> for Total {fn product<I:Iterable<Item=i32>>(source:I)->Self {print("product");val value=source.iter().fold(1,|a,n|{print("combine");a*n});Total{value}}}
fn main()->i32 {val total:Total = [20,22].iter().sum();total.value}
fn inner()->i32 {[6,7].iter().product()}
fn fail()->i8 {[127i8,1i8].iter().sum()}
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
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            let token = CancellationToken::default();
            let cancel = token.clone();
            let mut runtime = runtime();
            runtime
                .register_host_function(HostFunction::new(standard_log(), move |context, args| {
                    sink.borrow_mut().push(args[0].clone());
                    if args == [Value::Str("combine".into())] {
                        if cancellation {
                            cancel.cancel();
                        } else {
                            let root = context.runtime().execution_root().unwrap();
                            assert_eq!(
                                crate::reenter(context, &root, inner, &[]).unwrap().value(),
                                Value::I32(42)
                            );
                            assert!(crate::reenter(context, &root, fail, &[]).is_err());
                            assert_eq!(
                                context.runtime().resources().counters().current_call_depth,
                                3
                            );
                        }
                    }
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::Unit)
                }))
                .unwrap();
            let loaded = runtime
                .load_program("aggregate-reentry", route(&program, encoded))
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
                assert_eq!(
                    *effects.borrow(),
                    [Value::Str("sum".into()), Value::Str("combine".into())]
                );
            } else {
                assert_eq!(result.unwrap().return_value, Value::I32(42));
                assert_eq!(
                    *effects.borrow(),
                    [
                        Value::Str("sum".into()),
                        Value::Str("combine".into()),
                        Value::Str("combine".into())
                    ]
                );
            }
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
fn checked_numeric_traps_match_trait_entrypoints_and_release_every_cut() {
    for (scalar, method, items) in [
        ("i8", "sum", "127i8,1i8"),
        ("i8", "sum", "-128i8,-1i8"),
        ("i16", "sum", "32767i16,1i16"),
        ("i16", "sum", "-32768i16,-1i16"),
        ("u8", "sum", "255u8,1u8"),
        ("u16", "sum", "65535u16,1u16"),
        ("u32", "sum", "4294967295u32,1u32"),
        ("i32", "sum", "2147483647,1"),
        ("i64", "sum", "9223372036854775807i64,1i64"),
        ("u64", "sum", "18446744073709551615u64,1u64"),
        ("i8", "product", "-128i8,-1i8"),
        ("i16", "product", "32767i16,2i16"),
        ("u8", "product", "255u8,2u8"),
        ("u16", "product", "65535u16,2u16"),
        ("u32", "product", "4294967295u32,2u32"),
        ("i32", "product", "2147483647,2"),
        ("i64", "product", "9223372036854775807i64,2i64"),
        ("u64", "product", "18446744073709551615u64,2u64"),
    ] {
        let program = compile_test_bytecode(&format!(
            r#"
fn original()->{scalar} {{{scalar}::{method}([{items}].iter().map(|n|{{print("item");n}}))}}
fn migrated()->{scalar} {{[{items}].iter().map(|n|{{print("item");n}}).{method}()}}
fn ready()->i32 {{7}}
"#
        ));
        for encoded in [false, true] {
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            let base = Rc::new(Cell::new(0));
            let root_base = base.clone();
            let mut runtime = runtime();
            runtime
                .register_host_function(HostFunction::new(standard_log(), move |context, args| {
                    sink.borrow_mut().push((
                        args[0].clone(),
                        context.runtime().resources().counters().instruction_steps
                            - root_base.get(),
                    ));
                    Ok(Value::Unit)
                }))
                .unwrap();
            let loaded = runtime
                .load_program("aggregate-overflow", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(runtime);
            base.set(vm.runtime().resources().counters().instruction_steps);
            let Err(original) = vm.execute(&loaded, "original") else {
                panic!("expected overflow")
            };
            assert_eq!(trap(&original).0, RuntimeErrorKind::ScriptTrap);
            let narrow = matches!(scalar, "i8" | "i16" | "u8" | "u16" | "u32");
            assert_eq!(trap(&original).2, narrow);
            assert_eq!(
                trap(&original).1,
                if narrow {
                    "std::debug::assert: debug.assert failed: integer overflow"
                } else {
                    "integer overflow"
                }
            );
            let steps = vm.runtime().resources().counters().instruction_steps - base.get();
            let baseline = effects.borrow().clone();
            assert_eq!(baseline.len(), 2);
            assert_clean(&vm);
            for limit in 0..=steps {
                effects.borrow_mut().clear();
                base.set(vm.runtime().resources().counters().instruction_steps);
                let mut options = vm.runtime().execution_options();
                options.resources.max_instruction_steps = Some(limit);
                let session = vm.runtime().begin_execution(&loaded, options).unwrap();
                let Err(error) = vm.execute(&loaded, "migrated") else {
                    panic!("expected overflow or limit")
                };
                if limit == steps {
                    assert_eq!(trap(&error), trap(&original));
                    let trace = error.trace().unwrap();
                    assert_eq!(trace.frames.len(), 1);
                    assert_eq!(trace.frames[0].function_name, "migrated");
                } else {
                    assert!(matches!(error.cause(), VmError::RuntimeError(error)
                        if error.kind() == RuntimeErrorKind::ResourceLimitExceeded));
                }
                assert_eq!(session.counters().instruction_steps, limit);
                let expected: Vec<_> = baseline
                    .iter()
                    .filter(|(_, step)| *step <= limit)
                    .cloned()
                    .collect();
                assert_eq!(*effects.borrow(), expected, "{scalar} {method} {limit}");
                drop(session);
                assert_clean(&vm);
            }
            assert_eq!(
                vm.execute(&loaded, "ready").unwrap().return_value,
                Value::I32(7)
            );
        }
    }
}

fn trap(error: &VmError) -> (RuntimeErrorKind, &str, bool) {
    match error.cause() {
        VmError::RuntimeError(error) => (error.kind(), error.message(), false),
        VmError::BuiltinError(error) => (error.kind(), error.message(), true),
        other => panic!("expected a script trap, got {other:?}"),
    }
}
