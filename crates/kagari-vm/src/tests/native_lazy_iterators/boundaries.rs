//! Shared-frame cancellation, allocation cuts and successful/failing host reentry.
use super::cases::TYPES;
use crate::tests::{common::compile_test_bytecode, native_destinations::boundaries::lifecycle};
#[test]
fn lazy_iterators_reenter_cancel_and_exhaust_every_allocation_cut() {
    for (name, expression, item, expected) in [
        (
            "map",
            "fixture.iter().map(|x|{print(\"visit\");x*2})",
            "i32",
            "2,4,6",
        ),
        (
            "filter",
            "fixture.iter().filter(|x|{print(\"visit\");x%2==1})",
            "i32",
            "1,3",
        ),
        (
            "filter_map",
            "fixture.iter().filter_map(|x|{print(\"visit\");if x==2{None}else{Some(x*2)}})",
            "i32",
            "2,6",
        ),
        ("take", "fixture.iter().take(2usize)", "i32", "1,2"),
        ("skip", "fixture.iter().skip(2usize)", "i32", "3"),
        (
            "enumerate",
            "fixture.iter().enumerate()",
            "(usize,i32)",
            "(0usize,1),(1usize,2),(2usize,3)",
        ),
        (
            "zip",
            "fixture.iter().zip(Sequence{items:[20,22]})",
            "(i32,i32)",
            "(1,20),(2,22)",
        ),
        (
            "chain",
            "fixture.iter().chain(Sequence{items:[20,22]})",
            "i32",
            "1,2,3,20,22",
        ),
        (
            "take_while",
            "fixture.iter().take_while(|x|{print(\"visit\");x<2})",
            "i32",
            "1",
        ),
        (
            "skip_while",
            "fixture.iter().skip_while(|x|{print(\"visit\");x<2})",
            "i32",
            "2,3",
        ),
        (
            "inspect",
            "fixture.iter().inspect(|x|{print(\"visit\");})",
            "i32",
            "1,2,3",
        ),
        ("fuse", "fixture.iter().fuse()", "i32", "1,2,3"),
        (
            "flat_map",
            "fixture.iter().flat_map(|x|{print(\"visit\");Sequence{items:[x,x+1]}})",
            "i32",
            "1,2,2,3,3,4",
        ),
        (
            "flatten",
            "fixture.iter().map(|x|{print(\"visit\");Sequence{items:[x,x+1]}}).flatten()",
            "i32",
            "1,2,2,3,3,4",
        ),
        (
            "windows",
            "Proxy{items:fixture}.windows(2usize).flat_map(|piece|piece)",
            "i32",
            "1,2,2,3",
        ),
        (
            "chunks",
            "Proxy{items:fixture}.chunks(2usize).flat_map(|piece|piece)",
            "i32",
            "1,2,3",
        ),
    ] {
        let program = compile_test_bytecode(&format!(
            r#"{TYPES}
fn setup()->ArrayList<i32>{{[1,2,3]}}
fn main(fixture:ArrayList<i32>)->i32{{print("start");val iterator={expression};print("constructed");val values:ArrayList<{item}> =iterator.collect();val expected:ArrayList<{item}> =[{expected}];std::debug::assert(values.len()==expected.len() && values.iter().zip(expected).all(|pair|pair[0]==pair[1]),"values");print("done");42}}
fn inner()->i32{{42}}fn fail()->i32{{val x=2147483647;x+1}}fn ready()->i32{{7}}
"#
        ));
        lifecycle(&program, name);
    }
}

#[test]
fn indexed_lazy_failures_preserve_categories_and_completed_effects() {
    use super::runtime;
    use crate::{error::VmError, vm::Vm};
    use kagari_bytecode::artifact::KbcArtifact;
    use kagari_common::host_interface::standard_log;
    use kagari_runtime::{host::HostFunction, value::Value};
    use std::{cell::RefCell, rc::Rc};
    let program = compile_test_bytecode(
        r#"
struct Inconsistent{val items:ArrayList<i32>}
impl Iterable for Inconsistent{type Item=i32;type Iter=Iter<i32>;fn iter(self)->Iter<i32>{print("iter");self.items.iter()}}
impl Index<usize> for Inconsistent{type Output=i32;fn index(self,i:usize)->i32{self.items[i]}}
impl List<i32> for Inconsistent{fn len(self)->usize{print("len");3usize}fn is_empty(self)->bool{false}fn get(self,i:usize)->Option<i32>{print("get");self.items.get(i)}}
fn zero()->i32{print("before");Inconsistent{items:[20]}.windows(0usize).count();42}
fn windows()->i32{print("before");Inconsistent{items:[20]}.windows(2usize).count();42}
fn chunks()->i32{print("before");Inconsistent{items:[20]}.chunks(2usize).count();42}
fn healthy()->i32{42}
"#,
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
        let labels = Rc::new(RefCell::new(Vec::new()));
        let sink = labels.clone();
        let mut rt = runtime();
        rt.register_host_function(HostFunction::new(standard_log(), move |_, args| {
            let Value::Str(label) = &args[0] else {
                panic!()
            };
            sink.borrow_mut().push(label.clone());
            Ok(Value::Unit)
        }))
        .unwrap();
        let loaded = rt.load_program("indexed-failures", program).unwrap();
        let mut vm = Vm::new(rt);
        for entry in ["zero", "windows", "chunks"] {
            labels.borrow_mut().clear();
            let error = vm.execute(&loaded, entry).unwrap_err();
            if entry == "zero" {
                assert!(
                    matches!(error.cause(), VmError::BuiltinError(_)),
                    "{error:?}"
                );
                let VmError::BuiltinError(error) = error.cause() else {
                    panic!("zero size error")
                };
                assert!(
                    error
                        .message()
                        .contains("window or chunk size must be nonzero")
                );
                assert_eq!(*labels.borrow(), ["before"]);
            } else {
                assert!(
                    matches!(
                        error.cause(),
                        VmError::TypeMismatch("standard enum payload variant")
                    ),
                    "{error:?}"
                );
                assert_eq!(*labels.borrow(), ["before", "iter", "len", "get", "get"]);
            }
            assert_eq!(vm.runtime().gc().active_roots(), 0);
            assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
            assert!(!vm.runtime().is_quarantined());
            assert_eq!(
                vm.execute(&loaded, "healthy").unwrap().return_value,
                Value::I32(42)
            );
            vm.runtime().collect_garbage().unwrap();
            assert_eq!(vm.runtime().gc().allocated_objects(), 0);
        }
    }
}

#[test]
fn nested_native_steps_enforce_the_shared_call_depth_budget() {
    use super::runtime;
    use crate::{error::VmError, vm::Vm};
    use kagari_common::host_interface::standard_log;
    use kagari_runtime::{error::RuntimeErrorKind, host::HostFunction, value::Value};
    let program = compile_test_bytecode(&format!(
        r#"{TYPES}
fn main()->i32{{val iterator=[1,2,3].iter().map(|x|x).enumerate().filter_map(|pair|Some(pair[1])).take(3usize).skip(0usize).take_while(|x|true).skip_while(|x|false).inspect(|x|{{print("visit");}}).zip(Sequence{{items:[10,20,30]}}).map(|pair|pair[0]).chain(Sequence{{items:[10]}}).flat_map(|x|Proxy{{items:[x]}}.chunks(1usize).flat_map(|piece|piece)).map(|x|Sequence{{items:[x]}}).flatten().fuse();val result:ArrayList<i32> =iterator.collect();std::debug::assert(result.len()==4usize,"depth");42}}
"#
    ));
    let mut rt = runtime();
    rt.register_host_function(HostFunction::new(standard_log(), |_, _| Ok(Value::Unit)))
        .unwrap();
    let loaded = rt.load_program("native-depth", program).unwrap();
    let mut vm = Vm::new(rt);
    let session = vm
        .runtime()
        .begin_execution(&loaded, vm.runtime().execution_options())
        .unwrap();
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
    let depth = session.counters().peak_call_depth;
    drop(session);
    assert!(depth > 10);
    for limit in 1..=depth {
        let mut options = vm.runtime().execution_options();
        options.resources.max_call_depth = Some(limit);
        let session = vm.runtime().begin_execution(&loaded, options).unwrap();
        let result = vm.execute(&loaded, "main");
        if limit < depth {
            assert!(
                matches!(result,Err(ref error) if matches!(error.cause(),VmError::RuntimeError(cause) if cause.kind()==RuntimeErrorKind::ResourceLimitExceeded)),
                "depth {limit}: {result:?}"
            );
        } else {
            assert_eq!(result.unwrap().return_value, Value::I32(42));
        }
        drop(session);
        assert_eq!(vm.runtime().gc().active_roots(), 0);
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
        assert!(!vm.runtime().is_quarantined());
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(vm.runtime().gc().allocated_objects(), 0);
    }
}
