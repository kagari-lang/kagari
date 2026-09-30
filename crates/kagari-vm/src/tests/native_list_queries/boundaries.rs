use super::runtime;
use crate::{Vm, VmError, tests::common::compile_test_bytecode};
use kagari_bytecode::{BytecodeProgram, KbcArtifact};
use kagari_common::cancellation::CancellationToken;
use kagari_common::host_interface::standard_log;
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
const TYPES: &str = r#"
struct Sequence<T> {val items:ArrayList<T>}
impl<T> Iterable for Sequence<T> {type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{print("iter");self.items.iter()}}
impl<T> Index<usize> for Sequence<T> {type Output=T;fn index(self,index:usize)->T {self.items[index]}}
impl<T> List<T> for Sequence<T> {fn len(self)->usize{print("len");self.items.len()}fn is_empty(self)->bool{self.items.is_empty()}fn get(self,index:usize)->Option<T>{print("get");self.items.get(index)}}
struct Rank {val value:ArrayList<i32>}
impl PartialEq for Rank {fn eq(self,other:Self)->bool{self.value[0]==other.value[0]}}
impl Eq for Rank {}
impl PartialOrd for Rank {fn partial_cmp(self,other:Self)->Option<Ordering>{self.value[0].partial_cmp(other.value[0])}}
impl Ord for Rank {fn cmp(self,other:Self)->Ordering{print("cmp");val absent:Option<i32> =None;self.value[0].cmp(absent.unwrap_or_else(||other.value[0]))}}
"#;
#[test]
fn dynamic_queries_reenter_and_cancel_at_each_protocol_callback() {
    for (call, labels) in [
        ("source.first()", vec!["get"]),
        ("source.last()", vec!["iter", "len", "get"]),
        (
            "source.binary_search(Rank{value:[2]})",
            vec!["iter", "len", "get", "cmp"],
        ),
    ] {
        let program = compile_test_bytecode(&format!(
            r#"{TYPES}
fn main()->i32 {{val source:List<Rank> =Sequence{{items:[Rank{{value:[2]}}]}};val result={call};42}}
fn inner()->i32 {{[7].last().unwrap_or(0)}}
fn fail()->i32 {{[1].get(2usize).unwrap_or_else(||{{val x=2147483647;x+1}})}}
fn ready()->i32 {{7}}
"#
        ));
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
            for cancellation in std::iter::once(None).chain(labels.iter().copied().map(Some)) {
                let token = CancellationToken::default();
                let cancel = token.clone();
                let effects = Rc::new(RefCell::new(Vec::new()));
                let sink = effects.clone();
                let mut runtime = runtime();
                runtime
                    .register_host_function(HostFunction::new(
                        standard_log(),
                        move |context, args| {
                            let Value::Str(label) = &args[0] else {
                                panic!()
                            };
                            sink.borrow_mut().push(label.clone());
                            if cancellation == Some(label.as_str()) {
                                cancel.cancel();
                            } else if cancellation.is_none() {
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
                        },
                    ))
                    .unwrap();
                let loaded = runtime
                    .load_program("list-reentry", route(&program, encoded))
                    .unwrap();
                let mut options = runtime.execution_options();
                options.cancellation = token;
                let session = runtime.begin_execution(&loaded, options).unwrap();
                let mut vm = Vm::new(runtime);
                let result = vm.execute(&loaded, "main");
                if let Some(label) = cancellation {
                    assert!(
                        matches!(result,Err(VmError::RuntimeError(error)) if error.kind()==RuntimeErrorKind::Cancelled)
                    );
                    assert_eq!(
                        *effects.borrow(),
                        labels[..=labels.iter().position(|value| *value == label).unwrap()]
                    );
                } else {
                    assert_eq!(result.unwrap().return_value, Value::I32(42));
                    assert_eq!(*effects.borrow(), labels);
                }
                drop(session);
                clean(&vm);
                assert_eq!(
                    vm.execute(&loaded, "ready").unwrap().return_value,
                    Value::I32(7)
                );
            }
        }
    }
}
#[test]
fn list_protocol_traps_keep_script_origins() {
    for (call, method) in [
        ("source.first()", "get"),
        ("source.last()", "iter"),
        ("source.last()", "len"),
        ("source.last()", "get"),
        ("source.binary_search(Rank{value:[2]})", "cmp"),
    ] {
        let marker = format!("print(\"{method}\");");
        let types = TYPES.replace(&marker, "val x=2147483647;val bad=x+1;");
        let program = compile_test_bytecode(&format!(
            r#"{types}fn main()->i32{{val source:List<Rank> =Sequence{{items:[Rank{{value:[2]}}]}};val out={call};0}}fn ready()->i32{{7}}"#
        ));
        for encoded in [false, true] {
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            let mut runtime = runtime();
            runtime
                .register_host_function(HostFunction::new(standard_log(), move |_, args| {
                    sink.borrow_mut().push(args[0].clone());
                    Ok(Value::Unit)
                }))
                .unwrap();
            let loaded = runtime
                .load_program("list-trap", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(runtime);
            let Err(VmError::RuntimeError(error)) = vm.execute(&loaded, "main") else {
                panic!("{method}")
            };
            assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
            let trace = error.trace().unwrap();
            assert_eq!(trace.frames.len(), 2);
            assert!(trace.frames[0].function_name.contains(method));
            assert_eq!(trace.frames[1].function_name, "main");
            clean(&vm);
            assert_eq!(
                vm.execute(&loaded, "ready").unwrap().return_value,
                Value::I32(7)
            );
        }
    }
}
#[test]
fn heap_results_share_payloads_and_native_guards_release_before_next_mutation() {
    let program = compile_test_bytecode(
        r#"
fn main()->i32 {val a=[20];val b=[22];val values=[a,b];val source:List<ArrayList<i32>> =values;
val first=source.first().unwrap_or([0]);val last=source.last().unwrap_or([0]);first.push(1);last.push(1);
std::debug::assert(a.len()==2usize && b.len()==2usize,"shared");values.push([0]);
std::debug::assert(["a","c","e"].binary_search("c")==Ok(1usize),"string");
std::debug::assert([Ordering::Less,Ordering::Equal,Ordering::Greater].binary_search(Ordering::Equal)==Ok(1usize),"order");
std::debug::assert([1,1,1].binary_search(1)==Ok(1usize),"duplicates");first[0]+last[0]}
"#,
    );
    for encoded in [false, true] {
        let mut runtime = runtime();
        let loaded = runtime
            .load_program("list-shared", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(&vm);
        assert!(vm.runtime().gc().stats().collections > 0);
    }
}
#[test]
fn inconsistent_list_get_retains_the_payload_failure_category() {
    let types = TYPES.replace("self.items.get(index)", "None");
    let program = compile_test_bytecode(&format!(
        "{types}fn main()->i32{{val source:List<i32> =Sequence{{items:[1]}};val result=source.binary_search(1);0}}fn ready()->i32{{7}}"
    ));
    for encoded in [false, true] {
        let mut runtime = runtime();
        runtime
            .register_host_function(HostFunction::new(standard_log(), |_, _| Ok(Value::Unit)))
            .unwrap();
        let loaded = runtime
            .load_program("inconsistent-list", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        let error = vm.execute(&loaded, "main").unwrap_err();
        assert!(
            matches!(
                error.cause(),
                VmError::TypeMismatch("standard enum payload variant")
            ),
            "{error:?}"
        );
        assert_eq!(
            error.trace().unwrap().frames.last().unwrap().function_name,
            "main"
        );
        clean(&vm);
        assert_eq!(
            vm.execute(&loaded, "ready").unwrap().return_value,
            Value::I32(7)
        );
    }
}

#[test]
fn multi_element_queries_reject_structural_alias_mutation_and_first_stays_unguarded() {
    let types = TYPES.replace(
        "self.items.get(index)",
        "self.items.push(self.items[0]);self.items.get(index)",
    );
    for call in ["last()", "binary_search(1)"] {
        let program = compile_test_bytecode(&format!(
            "{types}fn main()->i32{{val source:List<i32> =Sequence{{items:[1]}};val result=source.{call};0}}fn ready()->i32{{7}}"
        ));
        for encoded in [false, true] {
            let mut runtime = runtime();
            runtime
                .register_host_function(HostFunction::new(standard_log(), |_, _| Ok(Value::Unit)))
                .unwrap();
            let loaded = runtime
                .load_program("list-guard", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(runtime);
            let error = vm.execute(&loaded, "main").unwrap_err();
            let VmError::BuiltinError(cause) = error.cause() else {
                panic!("{error:?}")
            };
            assert_eq!(cause.kind(), RuntimeErrorKind::ScriptTrap);
            assert_eq!(
                cause.message(),
                "std::array::ArrayList::push: structural modification during iteration"
            );
            clean(&vm);
            assert_eq!(
                vm.execute(&loaded, "ready").unwrap().return_value,
                Value::I32(7)
            );
        }
    }
    let program = compile_test_bytecode(&format!(
        "{types}fn main()->i32{{val values=[42];val source:List<i32> =Sequence{{items:values}};val result=source.first().unwrap_or(0);std::debug::assert_eq(values.len(),2usize,\"first has no guard\");result}}"
    ));
    for encoded in [false, true] {
        let mut runtime = runtime();
        runtime
            .register_host_function(HostFunction::new(standard_log(), |_, _| Ok(Value::Unit)))
            .unwrap();
        let loaded = runtime
            .load_program("first-alias", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(&vm);
    }
}

#[test]
fn foreign_generic_list_and_ordering_use_their_defining_modules() {
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
pub struct Sequence<T> {pub val items:ArrayList<T>}
impl<T> Iterable for Sequence<T> {type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{self.items.iter()}}
impl<T> Index<usize> for Sequence<T> {type Output=T;fn index(self,index:usize)->T{self.items[index]}}
impl<T> List<T> for Sequence<T> {fn len(self)->usize{self.items.len()}fn is_empty(self)->bool{self.items.is_empty()}fn get(self,index:usize)->Option<T>{self.items.get(index)}}
pub struct Rank<T> {pub val value:T}
impl<T:PartialEq> PartialEq for Rank<T> {fn eq(self,other:Self)->bool{self.value==other.value}}
impl<T:Eq> Eq for Rank<T> {}
impl<T:PartialOrd> PartialOrd for Rank<T> {fn partial_cmp(self,other:Self)->Option<Ordering>{self.value.partial_cmp(other.value)}}
impl<T:Ord> Ord for Rank<T> {fn cmp(self,other:Self)->Ordering{self.value.cmp(other.value)}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Sequence,Rank};
fn positions<L:List<Rank<i32>>>(source:L)->i32{std::debug::assert_eq(source.binary_search(Rank{value:22}),Ok(1usize),"index");source.first().unwrap_or(Rank{value:0}).value+source.last().unwrap_or(Rank{value:0}).value}
fn main()->i32{val source=Sequence{items:[Rank{value:20},Rank{value:22}]};val dynamic:List<Rank<i32>> =source;std::debug::assert_eq(dynamic.binary_search(Rank{value:22}),Ok(1usize),"dynamic index");std::debug::assert_eq(dynamic.first().unwrap_or(Rank{value:0}).value+dynamic.last().unwrap_or(Rank{value:0}).value,42,"dynamic");positions(source)}
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
            .load_program("foreign-list", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(&vm);
    }
}

#[test]
fn inherited_list_views_resolve_their_required_parent_slots() {
    let program = compile_test_bytecode(
        r#"
fn main()->i32 {val values=[20,22];val child:MutableList<i32> =values;
std::debug::assert_eq(child.binary_search(21),Err(1usize),"parent search");
val answer=child.first().unwrap_or(0)+child.last().unwrap_or(0);child.push(0);answer}
"#,
    );
    for encoded in [false, true] {
        let mut runtime = runtime();
        let loaded = runtime
            .load_program("list-parent", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        clean(&vm);
    }
}
