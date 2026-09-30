use super::runtime;
use crate::{Vm, VmError, executor::Executor, tests::common::compile_test_bytecode};
use kagari_bytecode::{BytecodeProgram, KbcArtifact};
use kagari_common::{cancellation::CancellationToken, host_interface::standard_log};
use kagari_runtime::{RuntimeErrorKind, host::HostFunction, value::Value};
use std::{cell::RefCell, rc::Rc};

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
fn clean(vm: &Vm) {
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert!(!vm.runtime().is_quarantined());
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.runtime().gc().allocated_objects(), 0);
}

#[test]
fn initializer_callbacks_reenter_and_cancel_at_each_occurrence() {
    let program = compile_test_bytecode(
        r#"
struct Cell {var value:usize}
fn main()->i32{
 val shared=[Cell{value:42usize}];
 val values=ArrayList::from_fn(3usize,|i|{print("init");(Cell{value:i},shared)});
 std::debug::assert(values[2usize][0].value==2usize,"rooted item");
 values[0usize][1][0usize].value=7usize;
 std::debug::assert(values[2usize][1][0usize].value==7usize,"shared capture");
 print("done");42
}
fn inner()->i32{ArrayList::from_fn(2usize,|i|7)[1usize]}
fn fail()->i32{val x=2147483647;x+1}
fn ready()->i32{7}
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
    let labels = ["init", "init", "init", "done"];
    for encoded in [false, true] {
        for cancellation in 0..=labels.len() {
            let token = CancellationToken::default();
            let cancel = token.clone();
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            let mut rt = runtime();
            rt.register_host_function(HostFunction::new(standard_log(), move |context, args| {
                let Value::Str(label) = &args[0] else {
                    panic!()
                };
                sink.borrow_mut().push(label.clone());
                let depth = context.runtime().resources().counters().current_call_depth;
                if cancellation == sink.borrow().len() {
                    cancel.cancel();
                } else if cancellation == 0 {
                    let root = context.runtime().execution_root().unwrap();
                    assert_eq!(
                        crate::reenter(context, &root, inner, &[]).unwrap().value(),
                        Value::I32(7)
                    );
                    assert!(crate::reenter(context, &root, fail, &[]).is_err());
                    assert_eq!(
                        context.runtime().resources().counters().current_call_depth,
                        depth
                    );
                }
                context.runtime().collect_garbage().unwrap();
                Ok(Value::Unit)
            }))
            .unwrap();
            let loaded = rt
                .load_program("initializer-reentry", route(&program, encoded))
                .unwrap();
            let mut options = rt.execution_options();
            options.cancellation = token;
            let session = rt.begin_execution(&loaded, options).unwrap();
            let mut vm = Vm::new(rt);
            let result = vm.execute(&loaded, "main");
            if cancellation == 0 {
                assert_eq!(result.unwrap().return_value, Value::I32(42));
                assert_eq!(*effects.borrow(), labels);
            } else {
                assert!(
                    matches!(result,Err(VmError::RuntimeError(error)) if error.kind()==RuntimeErrorKind::Cancelled)
                );
                assert_eq!(*effects.borrow(), labels[..cancellation]);
            }
            drop(session);
            clean(&vm);
            assert_eq!(
                vm.execute(&loaded, "ready").unwrap().return_value,
                Value::I32(7)
            );
            clean(&vm);
        }
    }
}

#[test]
fn initializer_traps_and_unbounded_counts_preserve_effects_and_cleanup() {
    for trap in [true, false] {
        let body = if trap {
            "ArrayList::from_fn(3usize,|i|{print(\"init\");val a=[i];if i==1usize{val x=2147483647;x+1;}a})"
        } else {
            "ArrayList::from_fn(18446744073709551615usize,|i|{print(\"init\");[i]})"
        };
        let program = compile_test_bytecode(&format!(
            "fn main(){{val values={body};}}fn ready()->i32{{7}}"
        ));
        for encoded in [false, true] {
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            let mut rt = runtime();
            rt.register_host_function(HostFunction::new(standard_log(), move |_, _| {
                sink.borrow_mut().push("init");
                Ok(Value::Unit)
            }))
            .unwrap();
            let loaded = rt
                .load_program("initializer-trap", route(&program, encoded))
                .unwrap();
            let mut options = rt.execution_options();
            options.resources.max_instruction_steps = Some(80);
            let session = rt.begin_execution(&loaded, options).unwrap();
            let mut vm = Vm::new(rt);
            let error = vm.execute(&loaded, "main").unwrap_err();
            let VmError::RuntimeError(cause) = error.cause() else {
                panic!("{error:?}")
            };
            assert_eq!(
                cause.kind(),
                if trap {
                    RuntimeErrorKind::ScriptTrap
                } else {
                    RuntimeErrorKind::ResourceLimitExceeded
                }
            );
            if trap {
                assert_eq!(*effects.borrow(), ["init", "init"]);
                assert_eq!(error.trace().unwrap().frames.len(), 2);
                assert_eq!(
                    error.trace().unwrap().frames.last().unwrap().function_name,
                    "main"
                );
            } else {
                assert!(!effects.borrow().is_empty());
                assert_eq!(session.counters().instruction_steps, 80);
            }
            drop(session);
            clean(&vm);
            assert_eq!(
                vm.execute(&loaded, "ready").unwrap().return_value,
                Value::I32(7)
            );
            clean(&vm);
        }
    }
}

#[test]
fn initializer_allocation_limits_preserve_entry_failure_and_completed_callbacks() {
    let program = compile_test_bytecode(
        r#"
fn main(count:usize,initializer:fn(usize)->usize)->ArrayList<usize>{ArrayList::from_fn(count,initializer)}
fn initialize(i:usize)->usize{print("init");i}
"#,
    );
    let root = &program.modules[program.root.index()];
    let entry = root.functions.iter().find(|f| f.name == "main").unwrap().id;
    let initialize = root
        .functions
        .iter()
        .find(|f| f.name == "initialize")
        .unwrap()
        .id;
    for encoded in [false, true] {
        for limit in 0..=4 {
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            let mut rt = runtime();
            rt.register_host_function(HostFunction::new(standard_log(), move |_, _| {
                sink.borrow_mut().push("init");
                Ok(Value::Unit)
            }))
            .unwrap();
            let loaded = rt
                .load_program("initializer-allocation", route(&program, encoded))
                .unwrap();
            let callback = rt.make_closure(&loaded, initialize, vec![]).unwrap();
            let mut options = rt.execution_options();
            options.resources.max_allocation_units = Some(limit);
            let session = rt.begin_execution(&loaded, options).unwrap();
            let result = Executor::new(&rt, &loaded, entry, &[Value::U64(3), callback])
                .unwrap()
                .run();
            if limit == 4 {
                let Value::Array(id) = result.unwrap() else {
                    panic!()
                };
                assert_eq!(
                    rt.gc().array_snapshot(id).unwrap(),
                    [Value::U64(0), Value::U64(1), Value::U64(2)]
                );
                assert_eq!(*effects.borrow(), ["init", "init", "init"]);
                assert_eq!(session.counters().allocation_units, 4);
            } else {
                let error = result.unwrap_err();
                let VmError::RuntimeError(cause) = error.cause() else {
                    panic!()
                };
                assert_eq!(
                    cause.kind(),
                    RuntimeErrorKind::ResourceLimitExceeded,
                    "{encoded} {limit}: {error:?}"
                );
                assert_eq!(error.trace().unwrap().frames.len(), 1);
                assert_eq!(
                    error.trace().unwrap().frames.last().unwrap().function_name,
                    "main"
                );
                assert_eq!(effects.borrow().len(), limit);
                if limit == 0 {
                    assert_eq!(session.counters().instruction_steps, 3);
                }
            }
            drop(session);
            clean(&Vm::new(rt));
        }
    }
}

#[test]
fn foreign_generic_initializers_pin_callback_values_and_native_owners() {
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::{SourceDatabase, SourceLayer},
    };
    use kagari_compiler::{
        bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir,
    };
    use kagari_hir::analysis::AnalysisDatabase;
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub struct Cell {pub var value:i32}
pub struct Factory<T> {pub val seed:T}
pub trait Initialize<T> {fn initializer(self)->fn(usize)->T;}
pub trait Construct<T> {fn build(self,count:usize)->ArrayList<T>;}
impl<T> Initialize<T> for Factory<T> {fn initializer(self)->fn(usize)->T {|i|self.seed}}
impl<T> Construct<T> for Factory<T> {fn build(self,count:usize)->ArrayList<T>{ArrayList::from_fn(count,self.initializer())}}

"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Cell,Factory,Initialize,Construct};
fn main()->i32{
 val factory=Factory{seed:Cell{value:21}};
 val initializer=factory.initializer();
 val first=factory.build(2usize);
 val second=ArrayList::from_fn(2usize,initializer);
 first[0usize].value=20;
 std::debug::assert(second[1usize].value==20,"shared foreign captures");
 val nested=Factory{seed:[first[0usize],second[0usize]]}.build(2usize);
 nested[0usize][0usize].value+22
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
            .load_program("foreign-initializer", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(&vm);
    }
}
