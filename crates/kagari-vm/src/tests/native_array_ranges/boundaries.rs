use super::runtime;
use crate::{
    error::VmError, executor::Executor, reentry::reenter, tests::common::compile_test_bytecode,
    vm::Vm,
};

use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::{cancellation::CancellationToken, host_interface::standard_log};
use kagari_runtime::{error::RuntimeErrorKind, host::HostFunction, value::Value};
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
fn bound_callbacks_reenter_cancel_and_preserve_rooted_shared_results() {
    for method in ["copy", "remove"] {
        let action = if method == "copy" {
            "a.copy_within(r,1usize);std::debug::assert(a[1usize].value==20,\"overlap\");a[1usize].value=19;"
        } else {
            "val out:List<Cell> =a.remove_range(r);std::debug::assert(out[0usize].value==20 && a[0usize].value==22,\"removed\");out[0usize].value=19;"
        };
        let program = compile_test_bytecode(&format!(
            r#"
struct Cell{{var value:i32}}
struct Region{{val a:ArrayList<Cell>}}
impl RangeBounds<usize> for Region{{fn start_bound(self)->Bound<usize>{{print("start");Bound::Included(0usize)}}fn end_bound(self)->Bound<usize>{{print("end");Bound::Excluded(2usize)}}}}
fn main()->i32{{val seed=Cell{{value:20}};val a=[seed,Cell{{value:21}},Cell{{value:22}}];val r=Region{{a:a}};{action}std::debug::assert(seed.value==19,"shared objects");print("committed");42}}
fn inner()->i32{{val a=[7,8];a.copy_within(..=0usize,1usize);val removed:List<i32> =a.remove_range(..=0usize);removed[0usize]}}
fn fail()->i32{{val x=2147483647;x+1}}fn ready()->i32{{7}}
"#
        ));
        let root = &program.modules[program.root.index()];
        let inner = root
            .functions
            .iter()
            .find(|f| f.name == "inner")
            .unwrap()
            .id;
        let fail = root.functions.iter().find(|f| f.name == "fail").unwrap().id;
        for encoded in [false, true] {
            for cancellation in 0..=3 {
                let token = CancellationToken::default();
                let cancel = token.clone();
                let calls = Rc::new(RefCell::new(Vec::new()));
                let sink = calls.clone();
                let mut rt = runtime();
                rt.register_host_function(HostFunction::new(standard_log(), move |ctx, args| {
                    let Value::Str(label) = &args[0] else {
                        panic!()
                    };
                    sink.borrow_mut().push(label.clone());
                    if cancellation == sink.borrow().len() {
                        cancel.cancel();
                    } else if cancellation == 0 {
                        let depth = ctx.runtime().resources().counters().current_call_depth;
                        let root = ctx.runtime().execution_root().unwrap();
                        assert_eq!(
                            reenter(ctx, &root, inner, &[]).unwrap().value(),
                            Value::I32(7)
                        );
                        assert!(reenter(ctx, &root, fail, &[]).is_err());
                        assert_eq!(
                            ctx.runtime().resources().counters().current_call_depth,
                            depth
                        );
                    }
                    ctx.runtime().collect_garbage().unwrap();
                    Ok(Value::Unit)
                }))
                .unwrap();
                let loaded = rt
                    .load_program("ranges-reentry", route(&program, encoded))
                    .unwrap();
                let mut options = rt.execution_options();
                options.cancellation = token;
                let session = rt.begin_execution(&loaded, options).unwrap();
                let mut vm = Vm::new(rt);
                let result = vm.execute(&loaded, "main");
                let labels = ["start", "end", "committed"];
                if cancellation == 0 {
                    assert_eq!(result.unwrap().return_value, Value::I32(42));
                    assert_eq!(*calls.borrow(), labels);
                } else {
                    assert!(
                        matches!(result,Err(VmError::RuntimeError(e)) if e.kind()==RuntimeErrorKind::Cancelled)
                    );
                    assert_eq!(*calls.borrow(), labels[..cancellation]);
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
}
#[test]
fn interval_allocation_failures_leave_target_uncommitted() {
    for method in [
        "copy_within(0usize..2usize,1usize)",
        "remove_range(1usize..3usize)",
    ] {
        let program = compile_test_bytecode(&format!("fn main(a:ArrayList<i32>){{a.{method};}}"));
        let entry = program.modules[program.root.index()]
            .functions
            .iter()
            .find(|f| f.name == "main")
            .unwrap()
            .id;
        for encoded in [false, true] {
            let mut maximum = 0;
            for phase in 0..2 {
                let limits: Vec<_> = if phase == 0 {
                    vec![None]
                } else {
                    (0..=maximum).map(Some).collect()
                };
                for limit in limits {
                    let mut rt = runtime();
                    let loaded = rt
                        .load_program("range-allocation", route(&program, encoded))
                        .unwrap();
                    let target = rt
                        .alloc_array(vec![Value::I32(20), Value::I32(21), Value::I32(22)])
                        .unwrap();
                    let root = rt.gc().root_value(Value::Array(target)).unwrap();
                    let mut options = rt.execution_options();
                    options.resources.max_allocation_units = limit;
                    let session = rt.begin_execution(&loaded, options).unwrap();
                    let result = Executor::new(&rt, &loaded, entry, &[Value::Array(target)])
                        .unwrap()
                        .run();
                    if limit.is_none() {
                        maximum = session.counters().allocation_units;
                        assert!(maximum >= 2);
                    }
                    if limit.is_none() || limit == Some(maximum) {
                        assert_eq!(result.unwrap(), Value::Unit);
                        let expected = if method.starts_with("copy") {
                            vec![20, 20, 21]
                        } else {
                            vec![20]
                        };
                        assert_eq!(
                            rt.gc().array_snapshot(target).unwrap(),
                            expected.into_iter().map(Value::I32).collect::<Vec<_>>()
                        );
                        assert_eq!(session.counters().allocation_units, maximum);
                    } else {
                        let error = result.unwrap_err();
                        let VmError::RuntimeError(cause) = error.cause() else {
                            panic!("{error:?}")
                        };
                        assert_eq!(cause.kind(), RuntimeErrorKind::ResourceLimitExceeded);
                        assert_eq!(
                            rt.gc().array_snapshot(target).unwrap(),
                            vec![Value::I32(20), Value::I32(21), Value::I32(22)]
                        );
                    }
                    drop(session);
                    drop(root);
                    clean(&Vm::new(rt));
                }
            }
        }
    }
}
#[test]
fn interval_failures_keep_bound_effects_and_validate_current_storage() {
    for method in ["copy", "remove"] {
        for failure in [
            "start",
            "end",
            "reversed",
            "overflow_start",
            "overflow_end",
            "outside",
            "destination",
            "guard",
            "shrink",
        ] {
            let lower = match failure {
                "start" => "val x=2147483647;x+1;Bound::Included(0usize)",
                "reversed" => "Bound::Included(2usize)",
                "overflow_start" => "Bound::Excluded(18446744073709551615usize)",
                _ => "Bound::Included(0usize)",
            };
            let upper = match failure {
                "end" => "val x=2147483647;x+1;Bound::Excluded(2usize)",
                "reversed" => "Bound::Excluded(1usize)",
                "overflow_end" => "Bound::Included(18446744073709551615usize)",
                "outside" => "Bound::Excluded(4usize)",
                "shrink" => "self.a.pop();Bound::Excluded(3usize)",
                _ => "Bound::Excluded(2usize)",
            };
            let dest = if failure == "destination" { 4 } else { 1 };
            let action = if method == "copy" {
                format!("a.copy_within(r,{dest}usize);")
            } else {
                "a.remove_range(r);".into()
            };
            let setup = if failure == "guard" {
                "val guard=a.iter();"
            } else {
                ""
            };
            let tail = if failure == "guard" {
                "std::debug::assert(guard.next().unwrap_or(0)==42,\"live guard\");"
            } else {
                ""
            };
            let program = compile_test_bytecode(&format!(
                r#"
struct Region{{val a:ArrayList<i32>}}
impl RangeBounds<usize> for Region{{fn start_bound(self)->Bound<usize>{{self.a[0usize]=42;print("start");{lower}}}fn end_bound(self)->Bound<usize>{{print("end");{upper}}}}}
fn main(a:ArrayList<i32>){{{setup}val r=Region{{a:a}};{action}{tail}}}
"#
            ));
            let entry = program.modules[program.root.index()]
                .functions
                .iter()
                .find(|f| f.name == "main")
                .unwrap()
                .id;
            for encoded in [false, true] {
                let effects = Rc::new(RefCell::new(Vec::new()));
                let sink = effects.clone();
                let mut rt = runtime();
                rt.register_host_function(HostFunction::new(standard_log(), move |_, args| {
                    let Value::Str(label) = &args[0] else {
                        panic!()
                    };
                    sink.borrow_mut().push(label.clone());
                    Ok(Value::Unit)
                }))
                .unwrap();
                let loaded = rt
                    .load_program("range-failure", route(&program, encoded))
                    .unwrap();
                let target = rt
                    .alloc_array(vec![Value::I32(20), Value::I32(21), Value::I32(22)])
                    .unwrap();
                let root = rt.gc().root_value(Value::Array(target)).unwrap();
                let session = rt.begin_execution(&loaded, rt.execution_options()).unwrap();
                let result = Executor::new(&rt, &loaded, entry, &[Value::Array(target)])
                    .unwrap()
                    .run();
                let success = (failure == "guard" && method == "copy")
                    || (failure == "destination" && method == "remove");
                let expected = if failure == "shrink" {
                    vec![42, 21]
                } else if success && method == "copy" {
                    vec![42, 42, 21]
                } else if success {
                    vec![22]
                } else {
                    vec![42, 21, 22]
                };
                assert_eq!(
                    rt.gc().array_snapshot(target).unwrap(),
                    expected.into_iter().map(Value::I32).collect::<Vec<_>>(),
                    "{method} {failure}"
                );
                if success {
                    assert_eq!(result.unwrap(), Value::Unit);
                } else {
                    let error = result.unwrap_err();
                    assert_eq!(
                        error.trace().unwrap().frames.last().unwrap().function_name,
                        "main"
                    );
                    if matches!(failure, "start" | "end") {
                        let VmError::RuntimeError(cause) = error.cause() else {
                            panic!("{error:?}")
                        };
                        assert_eq!(cause.kind(), RuntimeErrorKind::ScriptTrap);
                    } else {
                        if method == "copy" {
                            let VmError::RuntimeError(cause) = error.cause() else {
                                panic!("{error:?}")
                            };
                            assert_eq!(cause.kind(), RuntimeErrorKind::IndexOutOfBounds);
                        } else {
                            assert!(
                                matches!(error.cause(), VmError::BuiltinError(_)),
                                "{method} {failure} {error:?}"
                            );
                        }
                    }
                }
                assert_eq!(
                    *effects.borrow(),
                    if failure == "start" {
                        vec!["start"]
                    } else {
                        vec!["start", "end"]
                    }
                );
                drop(session);
                drop(root);
                clean(&Vm::new(rt));
            }
        }
    }
}
#[test]
fn foreign_generic_bounds_and_readonly_results_use_pinned_defining_modules() {
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
pub struct Region<T>{pub val a:ArrayList<T>,pub val lower:usize,pub val upper:usize}
impl<T> RangeBounds<usize> for Region<T>{fn start_bound(self)->Bound<usize>{Bound::Included(self.lower)}fn end_bound(self)->Bound<usize>{Bound::Excluded(self.upper)}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::Region;
struct Cell{var value:i32}
fn copy<T,R:RangeBounds<usize>>(a:ArrayList<T>,r:R,d:usize){a.copy_within(r,d);}
fn remove<T,R:RangeBounds<usize>>(a:ArrayList<T>,r:R)->List<T>{a.remove_range(r)}
fn main()->i32{
 val seed=Cell{value:20};val a=[seed,Cell{value:21},Cell{value:22},Cell{value:23}];
 copy(a,Region{a:a,lower:0usize,upper:2usize},2usize);
 val out:List<Cell> =remove(a,Region{a:a,lower:1usize,upper:3usize});
 std::debug::assert(out.len()==2usize && a.len()==2usize,"independent slots");
 out[1usize].value=19;out[0usize].value=23;
 std::debug::assert(a[0usize].value==19 && a[1usize].value==23 && seed.value==19,"shared payloads");
 a.push(Cell{value:7});std::debug::assert(out.len()==2usize,"released mutation guard");
 a[0usize].value+out[0usize].value
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
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let program = lower_program_to_bytecode(&mir).unwrap();
    for encoded in [false, true] {
        let mut rt = runtime();
        let loaded = rt
            .load_program("foreign-ranges", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(&vm);
    }
}
