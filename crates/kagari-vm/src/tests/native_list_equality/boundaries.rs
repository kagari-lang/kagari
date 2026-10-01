use super::{cases::TYPES, runtime};
use crate::{error::VmError, reentry::reenter, tests::common::compile_test_bytecode, vm::Vm};

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
fn equality_composition_reenters_and_cancels_on_the_ordinary_stack() {
    for (call, labels) in [
        (
            "contains(Some(Key{value:42}))",
            vec!["iter", "len", "get", "eq"],
        ),
        (
            "starts_with(needle)",
            vec!["iter", "len", "iter", "len", "get", "get", "eq"],
        ),
        (
            "ends_with(needle)",
            vec!["iter", "len", "iter", "len", "get", "get", "eq"],
        ),
    ] {
        let program = compile_test_bytecode(&format!(
            r#"{TYPES}
fn main()->i32 {{val source:List<Option<Key<i32>>> =Sequence{{items:[Some(Key{{value:42}})]}};
val needle:List<Option<Key<i32>>> =Sequence{{items:[Some(Key{{value:42}})]}};
std::debug::assert(source.{call},"query");42}}
fn inner()->i32 {{if [1].contains(1) {{7}} else {{0}}}}
fn fail()->i32 {{val x=2147483647;x+1}}
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
            for cancellation in 0..=labels.len() {
                let token = CancellationToken::default();
                let cancel = token.clone();
                let effects = Rc::new(RefCell::new(Vec::new()));
                let sink = effects.clone();
                let mut rt = runtime();
                rt.register_host_function(HostFunction::new(
                    standard_log(),
                    move |context, args| {
                        let Value::Str(label) = &args[0] else {
                            panic!()
                        };
                        sink.borrow_mut().push(label.clone());
                        let depth = if label == "eq" { 3 } else { 2 };
                        assert_eq!(
                            context.runtime().resources().counters().current_call_depth,
                            depth
                        );
                        if cancellation == sink.borrow().len() {
                            cancel.cancel();
                        } else if cancellation == 0 {
                            let root = context.runtime().execution_root().unwrap();
                            assert_eq!(
                                reenter(context, &root, inner, &[]).unwrap().value(),
                                Value::I32(7)
                            );
                            assert!(reenter(context, &root, fail, &[]).is_err());
                            assert_eq!(
                                context.runtime().resources().counters().current_call_depth,
                                depth
                            );
                        }
                        context.runtime().collect_garbage().unwrap();
                        Ok(Value::Unit)
                    },
                ))
                .unwrap();
                let loaded = rt
                    .load_program("equality-reentry", route(&program, encoded))
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
            }
        }
    }
}

#[test]
fn composed_equality_traps_retain_core_and_selected_method_frames() {
    let types = TYPES.replace("print(\"eq\");", "val x=2147483647;val bad=x+1;");
    let program = compile_test_bytecode(&format!(
        r#"{types}
fn main()->i32{{val out=[Some(Key{{value:42}})].contains(Some(Key{{value:42}}));0}}
fn ready()->i32{{7}}"#
    ));
    for encoded in [false, true] {
        let mut rt = runtime();
        let loaded = rt
            .load_program("equality-trap", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(rt);
        let Err(VmError::RuntimeError(error)) = vm.execute(&loaded, "main") else {
            panic!()
        };
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        let trace = error.trace().unwrap();
        assert_eq!(trace.frames.len(), 3);
        assert!(trace.frames[0].function_name.contains("eq"));
        assert!(trace.frames[1].function_name.contains("$derived_PartialEq"));
        assert_eq!(trace.frames[2].function_name, "main");
        clean(&vm);
        assert_eq!(
            vm.execute(&loaded, "ready").unwrap().return_value,
            Value::I32(7)
        );
    }
}

#[test]
fn both_list_guards_and_inconsistent_payload_failures_release_on_traps() {
    for method in ["starts_with", "ends_with"] {
        for bad_source in [false, true] {
            for mutation in [false, true] {
                let types = TYPES
                    .replace(
                        "struct Sequence<T> {val items:ArrayList<T>}",
                        "struct Sequence<T> {val items:ArrayList<T>,val bad:bool}",
                    )
                    .replace(
                        "self.items.get(index)",
                        if mutation {
                            "if self.bad {self.items.push(self.items[0]);}self.items.get(index)"
                        } else {
                            "if self.bad {None} else {self.items.get(index)}"
                        },
                    );
                let program = compile_test_bytecode(&format!(
                    r#"{types}
fn main()->i32{{val source:List<i32> =Sequence{{items:[42],bad:{bad_source}}};
val needle:List<i32> =Sequence{{items:[42],bad:{}}};val out=source.{method}(needle);0}}
fn ready()->i32{{7}}"#,
                    !bad_source
                ));
                for encoded in [false, true] {
                    let mut rt = runtime();
                    rt.register_host_function(HostFunction::new(standard_log(), |_, _| {
                        Ok(Value::Unit)
                    }))
                    .unwrap();
                    let loaded = rt
                        .load_program("equality-guard", route(&program, encoded))
                        .unwrap();
                    let mut vm = Vm::new(rt);
                    let error = vm.execute(&loaded, "main").unwrap_err();
                    if mutation {
                        let VmError::BuiltinError(cause) = error.cause() else {
                            panic!("{error:?}")
                        };
                        assert_eq!(
                            cause.message(),
                            "std::array::ArrayList::push: structural modification during iteration"
                        );
                    } else {
                        assert!(
                            matches!(
                                error.cause(),
                                VmError::TypeMismatch("standard enum payload variant")
                            ),
                            "{error:?}"
                        );
                    }
                    clean(&vm);
                    assert_eq!(
                        vm.execute(&loaded, "ready").unwrap().return_value,
                        Value::I32(7)
                    );
                }
            }
        }
    }
}

#[test]
fn foreign_generic_equality_composition_uses_defining_modules() {
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
fn queries<L:List<Option<Rank<i32>>>>(source:L)->i32 {
std::debug::assert(source.contains(Some(Rank{value:22})),"static contains");
std::debug::assert(source.starts_with([Some(Rank{value:20})]),"static prefix");
std::debug::assert(source.ends_with([Some(Rank{value:22})]),"static suffix");42}
fn main()->i32 {
val source=Sequence{items:[Some(Rank{value:20}),Some(Rank{value:22})]};
val dynamic:List<Option<Rank<i32>>> =source;
std::debug::assert(dynamic.contains(Some(Rank{value:22})),"dynamic contains");
std::debug::assert(dynamic.starts_with([Some(Rank{value:20})]),"dynamic prefix");
std::debug::assert(dynamic.ends_with([Some(Rank{value:22})]),"dynamic suffix");
std::debug::assert([Rank{value:22}].contains(Rank{value:22}),"foreign leaf");queries(source)}
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
