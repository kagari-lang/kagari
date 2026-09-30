use super::{assert_clean, route, runtime};
use crate::{Vm, VmError, tests::common::compile_test_bytecode};
use kagari_common::{
    cancellation::CancellationToken,
    host_interface::standard_log,
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    RuntimeErrorKind,
    host::{HostError, HostFunction},
    value::Value,
};
use std::{cell::RefCell, rc::Rc};

#[test]
fn numeric_conversion_and_next_methods_materialize_in_their_defining_module() {
    let mut sources = SourceDatabase::default();
    let mut root = None;
    for (name, text) in [
        (
            "model",
            r#"
pub struct Counter<T> {pub val items:ArrayList<T>,pub var index:usize}
impl<T> Iterator for Counter<T> {type Item=T;fn next(self)->Option<T>{if self.index>=self.items.len(){None}else{val item=self.items[self.index];self.index+=1;Some(item)}}}
pub struct Wrap<T> {pub val values:ArrayList<T>}
impl<T> Iterable for Wrap<T> {type Item=T;type Iter=Counter<T>;fn iter(self)->Counter<T>{Counter{items:self.values,index:0}}}
"#,
        ),
        (
            "root",
            r#"
use pkg::model::{Wrap,Counter};
fn total<I:Iterable<Item=i32>>(source:I)->i32 {i32::sum(source)}
fn multiply<I:Iterable<Item=i32>>(source:I)->i32 {i32::product(source)}
fn main()->i32 {
    val dynamic:Iterable<Item=i32,Iter=Counter<i32>> = Wrap{values:[2,3,7]};
    std::debug::assert_eq(total(Wrap{values:[20,22]}),42,"foreign conversion");
    std::debug::assert_eq(multiply(dynamic),42,"foreign interface");42
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
    let program =
        lower_program_to_bytecode(&lower_program_to_mir(&checked, &Default::default()).unwrap())
            .unwrap();
    for encoded in [false, true] {
        let mut runtime = runtime();
        let loaded = runtime
            .load_program("foreign-numeric", route(&program, encoded))
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
fn numeric_conversion_callbacks_preserve_reentry_cancellation_and_traps() {
    let program = compile_test_bytecode(
        r#"
struct Wrap {val values:ArrayList<i32>}
impl Iterable for Wrap {type Item=i32;type Iter=Iter<i32>;fn iter(self)->Iter<i32>{print("iter");self.values.iter().map(|n|{print("item");n})}}
fn main()->i32 {val source:Iterable<Item=i32,Iter=Iter<i32>> = Wrap{values:[20,22]};i32::sum(source)}
fn inner()->i32 {i32::product([6,7])}
fn fail()->i8 {i8::sum([127i8,1i8])}
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
        for mode in ["reentry", "cancel", "trap"] {
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            let token = CancellationToken::default();
            let cancel = token.clone();
            let mut runtime = runtime();
            runtime
                .register_host_function(HostFunction::new(standard_log(), move |context, args| {
                    sink.borrow_mut().push(args[0].clone());
                    if args == [Value::Str("iter".into())] {
                        match mode {
                            "cancel" => cancel.cancel(),
                            "trap" => return Err(HostError::new("conversion failed")),
                            _ => {
                                let root = context.runtime().execution_root().unwrap();
                                assert_eq!(
                                    crate::reenter(context, &root, inner, &[]).unwrap().value(),
                                    Value::I32(42)
                                );
                                assert!(crate::reenter(context, &root, fail, &[]).is_err());
                                assert_eq!(
                                    context.runtime().resources().counters().current_call_depth,
                                    2
                                );
                            }
                        }
                    }
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::Unit)
                }))
                .unwrap();
            let loaded = runtime
                .load_program("numeric-reentry", route(&program, encoded))
                .unwrap();
            let mut options = runtime.execution_options();
            options.cancellation = token;
            let session = runtime.begin_execution(&loaded, options).unwrap();
            let mut vm = Vm::new(runtime);
            let result = vm.execute(&loaded, "main");
            if mode == "reentry" {
                assert_eq!(result.unwrap().return_value, Value::I32(42));
                assert_eq!(
                    *effects.borrow(),
                    [
                        Value::Str("iter".into()),
                        Value::Str("item".into()),
                        Value::Str("item".into())
                    ]
                );
            } else {
                let error = result.unwrap_err();
                if mode == "cancel" {
                    assert!(
                        matches!(error.cause(),VmError::RuntimeError(error) if error.kind()==RuntimeErrorKind::Cancelled)
                    );
                } else {
                    assert!(
                        matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::HostCallFailure && error.message() == "host call failed: conversion failed"),
                        "{error:?}"
                    );
                    let names: Vec<_> = error
                        .trace()
                        .unwrap()
                        .frames
                        .iter()
                        .map(|f| f.function_name.as_str())
                        .collect();
                    assert_eq!(names, ["iter", "main"]);
                }
                assert_eq!(*effects.borrow(), [Value::Str("iter".into())]);
            }
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
fn numeric_iteration_guards_mutable_aliases_of_converted_readonly_sources() {
    let program = compile_test_bytecode(
        r#"
struct Wrap {val values:ArrayList<i32>}
impl Iterable for Wrap {
    type Item=i32;type Iter=Iter<i32>;
    fn iter(self)->Iter<i32> {
        val view:[i32] = self.values;
        view.iter().map(|n|{print("item");self.values.push(99);n})
    }
}
fn main()->i32 {i32::sum(Wrap{values:[20,22]})}
fn ready()->i32 {7}
"#,
    );
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
            .load_program("numeric-guard", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        let error = vm.execute(&loaded, "main").unwrap_err();
        assert!(
            matches!(error.cause(), VmError::BuiltinError(error)
            if error.kind() == RuntimeErrorKind::ScriptTrap && error.message() == "std::array::ArrayList::push: structural modification during iteration"),
            "{error:?}"
        );
        assert_eq!(*effects.borrow(), [Value::Str("item".into())]);
        assert_clean(&vm);
        assert_eq!(
            vm.execute(&loaded, "ready").unwrap().return_value,
            Value::I32(7)
        );
    }
}
