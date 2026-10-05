#![cfg(feature = "source")]
use kagari_bytecode::program::BytecodeProgram;
use kagari_embed::{BytecodeArtifact, engine::KagariEngine};
use kagari_runtime::{Runtime, RuntimeConfig, value::Value};
use kagari_source::source::SourceFile;
use kagari_types::declaration::module::ModuleDecl;
use kagari_vm::vm::Vm;

fn prepare(engine: &KagariEngine, source: &str) -> BytecodeProgram {
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("generic-reload.kgr", source),
            Default::default(),
        )
        .unwrap();
    BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap())
        .unwrap()
        .program
}

#[test]
fn retained_generic_default_and_override_closures_pin_types_and_operations_after_reload() {
    let method =
        "fn keep<T: Ord>(self, a: T, b: T) -> fn() -> bool { || a.cmp(b) == Ordering::Less }";
    for implementation in [String::new(), method.to_owned()] {
        let source = format!(
            r#"use std::cmp::{{Ordering}};

trait Keep {{ {method} }}
impl Keep for i32 {{ {implementation} }}
struct Rank {{ val value: i32 }}
impl PartialEq for Rank {{ fn eq(self, other: Self) -> bool {{ self.value == other.value }} }}
impl Eq for Rank {{}}
impl PartialOrd for Rank {{ fn partial_cmp(self, other: Self) -> Option<Ordering> {{ self.value.partial_cmp(other.value) }} }}
impl Ord for Rank {{ fn cmp(self, other: Self) -> Ordering {{ self.value.cmp(other.value) }} }}
trait Run {{ fn run(self) -> bool; }}
struct Saved {{ val callback: fn() -> bool }}
impl Run for Saved {{ fn run(self) -> bool {{ (self.callback)() }} }}
fn make() -> Run {{
    val keeper: Keep = 0;
    Saved {{ callback: keeper.keep(Rank {{ value: 1 }}, Rank {{ value: 2 }}) }}
}}
"#
        );
        let engine = KagariEngine::default();
        let program = prepare(&engine, &source);
        let replacement = prepare(
            &engine,
            &source.replace("self.value.cmp(other.value)", "other.value.cmp(self.value)"),
        );
        let mut config = RuntimeConfig::default();
        config.gc.collection_threshold = Some(1);
        let mut runtime = {
            let mut registered = Runtime::new(config);
            kagari_runtime::native::module::NativeModule::install_all(
                &kagari_stdlib::modules().unwrap(),
                &mut registered,
            )
            .unwrap();
            registered
        };
        let old = runtime.load_program("generic-reload", program).unwrap();
        let contract = old
            .bytecode
            .trait_contracts
            .iter()
            .find(|contract| contract.abi.name == "Run")
            .unwrap();
        let run = ModuleDecl::method_id(
            &old.definitions()
                .resolve(contract.declaration)
                .unwrap()
                .to_path(),
            "run",
        );
        let old_key = old.key();
        let vm = Vm::new(runtime);
        let saved = vm.execute(&old, "make").unwrap().return_value;
        let root = vm.runtime().root_value(saved.clone()).unwrap();
        let current = vm
            .reload_program(&old, "generic-reload", replacement)
            .unwrap();
        drop(old);
        vm.runtime().collect_garbage().unwrap();
        assert!(vm.runtime().modules().loaded(old_key).is_some());
        let fresh = vm.execute(&current, "make").unwrap().return_value;
        let fresh_root = vm.runtime().root_value(fresh.clone()).unwrap();
        assert_eq!(
            vm.invoke_interface_method(&saved, &run, &[]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            vm.invoke_interface_method(&fresh, &run, &[]).unwrap(),
            Value::Bool(false)
        );
        drop((root, fresh_root));
        assert_eq!(vm.runtime().gc().active_roots(), 0);
        assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
        assert_eq!(
            vm.runtime()
                .modules()
                .retention_counts(old_key)
                .runtime_values,
            0
        );
    }
}

#[test]
fn retained_generic_try_calls_keep_the_selected_carrier_generation() {
    let source = r#"
use core::ops::{Try, FromResidual, ControlFlow};
use core::convert::Infallible;
enum Carrier { Value(i32), Stopped }
impl FromResidual<Option<Infallible>> for Carrier {
    fn from_residual(value:Option<Infallible>)->Self {Carrier::Stopped}
}
impl Try for Carrier {
    type Output = i32;
    type Residual = Option<Infallible>;
    fn from_output(value:i32)->Self {Carrier::Value(value)}
    fn branch(self)->ControlFlow<Option<Infallible>,i32> {
        match self { Carrier::Value(value) => ControlFlow::Continue(value), Carrier::Stopped => ControlFlow::Break(None) }
    }
}
trait Keep {
    fn keep<A: Try<Output = i32>, R: FromResidual<A::Residual>>(self, value:A, wrap:fn(i32)->R)->fn()->R { || wrap(value?) }
}
impl Keep for i32 {}
trait Run { fn run(self)->i32; }
struct Saved { val callback:fn()->Option<i32> }
impl Run for Saved { fn run(self)->i32 { match (self.callback)() {Some(value)=>value,None=>0} } }
fn make()->Run {val keeper:Keep=0; Saved {callback:keeper.keep(Carrier::Value(20),|value| Some(value+22))}}
fn make_stopped()->Run {val keeper:Keep=0; Saved {callback:keeper.keep(Carrier::Stopped,|value| Some(value+22))}}
"#;
    let engine = KagariEngine::default();
    let program = prepare(&engine, source);
    let replacement = prepare(
        &engine,
        &source.replace(
            "ControlFlow::Continue(value)",
            "ControlFlow::Continue(value + 1)",
        ),
    );
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = Runtime::new(config);
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let old = runtime.load_program("try-reload", program).unwrap();
    let contract = old
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Run")
        .unwrap();
    let run = ModuleDecl::method_id(
        &old.definitions()
            .resolve(contract.declaration)
            .unwrap()
            .to_path(),
        "run",
    );
    let old_key = old.key();
    let vm = Vm::new(runtime);
    let saved = vm.execute(&old, "make").unwrap().return_value;
    let root = vm.runtime().root_value(saved.clone()).unwrap();
    let stopped = vm.execute(&old, "make_stopped").unwrap().return_value;
    let stopped_root = vm.runtime().root_value(stopped.clone()).unwrap();
    let current = vm.reload_program(&old, "try-reload", replacement).unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().modules().loaded(old_key).is_some());
    let fresh = vm.execute(&current, "make").unwrap().return_value;
    let fresh_root = vm.runtime().root_value(fresh.clone()).unwrap();
    let fresh_stopped = vm.execute(&current, "make_stopped").unwrap().return_value;
    let fresh_stopped_root = vm.runtime().root_value(fresh_stopped.clone()).unwrap();
    assert_eq!(
        vm.invoke_interface_method(&saved, &run, &[]).unwrap(),
        Value::I32(42)
    );
    assert_eq!(
        vm.invoke_interface_method(&fresh, &run, &[]).unwrap(),
        Value::I32(43)
    );
    for value in [&stopped, &fresh_stopped] {
        assert_eq!(
            vm.invoke_interface_method(value, &run, &[]).unwrap(),
            Value::I32(0)
        );
    }
    drop((root, fresh_root, stopped_root, fresh_stopped_root));
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values,
        0
    );
}
