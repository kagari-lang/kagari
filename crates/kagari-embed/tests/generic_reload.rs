#![cfg(feature = "source")]
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_embed::{BytecodeArtifact, engine::KagariEngine};
use kagari_runtime::{Runtime, RuntimeConfig, native::module::NativeModule, value::Value};
use kagari_source::source::SourceFile;
use kagari_types::declaration::module::ModuleDecl;
use kagari_vm::vm::Vm;

fn prepare(engine: &KagariEngine, source: &str) -> DefinitionMetadata<BytecodeProgram> {
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("generic-reload.kgr", source),
            Default::default(),
        )
        .unwrap();
    let (definitions, artifact) =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap()
            .into_parts();
    DefinitionMetadata::checked(definitions, artifact.program, &Default::default()).unwrap()
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
        let run = ModuleDecl::method_id(contract.declaration, "run", old.definitions()).unwrap();
        let old_key = old.key();
        let vm = Vm::new(runtime);
        let saved = vm
            .execute(&old, "make")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result");
        let root = vm.runtime().root_value(saved).unwrap();
        let current = vm
            .reload_program(&old, "generic-reload", replacement)
            .unwrap();
        drop(old);
        vm.runtime().collect_garbage().unwrap();
        assert!(vm.runtime().modules().loaded(old_key).is_some());
        let fresh = vm
            .execute(&current, "make")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result");
        let fresh_root = vm.runtime().root_value(fresh).unwrap();
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
    let run = ModuleDecl::method_id(contract.declaration, "run", old.definitions()).unwrap();
    let old_key = old.key();
    let vm = Vm::new(runtime);
    let saved = vm
        .execute(&old, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(saved).unwrap();
    let stopped = vm
        .execute(&old, "make_stopped")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let stopped_root = vm.runtime().root_value(stopped).unwrap();
    let current = vm.reload_program(&old, "try-reload", replacement).unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().modules().loaded(old_key).is_some());
    let fresh = vm
        .execute(&current, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let fresh_root = vm.runtime().root_value(fresh).unwrap();
    let fresh_stopped = vm
        .execute(&current, "make_stopped")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let fresh_stopped_root = vm.runtime().root_value(fresh_stopped).unwrap();
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

#[test]
fn content_helper_retains_interface_iteration_generation_in_a_new_caller() {
    let source = r#"
use std::collections::{List, list_content_equal};
use std::iter::CollectionCursor;
use std::ops::Index;
struct Bag { val value: f32 }
impl Index<usize> for Bag { type Output = f32; fn index(self, index: usize) -> f32 { self.value } }
impl Iterable for Bag {
    type Item = f32;
    type Iter = CollectionCursor<f32>;
    fn iter(self) -> CollectionCursor<f32> { Vec::from([self.value]).iter() }
}
impl List<f32> for Bag {
    fn len(self) -> usize { 1usize }
    fn is_empty(self) -> bool { false }
    fn get(self, index: usize) -> Option<f32> { Some(self.value) }
}
trait Probe { fn same(self, a: List<f32>, b: List<f32>) -> bool; }
struct Checker {}
impl Probe for Checker { fn same(self, a: List<f32>, b: List<f32>) -> bool { list_content_equal(a, b) } }
fn make() -> List<f32> { Bag { value: 42.0f32 } }
fn probe() -> Probe { Checker {} }
"#;
    let engine = KagariEngine::default();
    let original = prepare(&engine, source);
    let replacement = prepare(
        &engine,
        &source.replace("Vec::from([self.value])", "Vec::from([0.0f32 / 0.0f32])"),
    );
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = Runtime::new(config);
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    let old = runtime.load_program("receiver-reload", original).unwrap();
    let old_key = old.key();
    let vm = Vm::new(runtime);
    let saved = vm.execute(&old, "make").unwrap().return_value;
    let current = vm
        .reload_program(&old, "receiver-reload", replacement)
        .unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().modules().loaded(old_key).is_some());
    let fresh = vm.execute(&current, "make").unwrap().return_value;
    let probe = vm.execute(&current, "probe").unwrap().return_value;
    let contract = current
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Probe")
        .unwrap();
    let method =
        ModuleDecl::method_id(contract.declaration, "same", current.definitions()).unwrap();
    for (root, expected) in [(&saved, true), (&fresh, false)] {
        let value = root.value(vm.runtime().gc()).unwrap();
        assert_eq!(
            vm.invoke_interface_method(
                &probe.value(vm.runtime().gc()).unwrap(),
                &method,
                &[value, value]
            )
            .unwrap(),
            Value::Bool(expected)
        );
    }
    drop((saved, fresh, probe));
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

#[test]
fn shared_collection_comparison_retains_the_callers_nominal_generation() {
    assert_shared_collection_comparison_reloading(SHARED_COLLECTION_SOURCE, false, |source| {
        source.replace("bool { true }", "bool { false }")
    });
}

#[test]
fn shared_collection_comparison_accepts_new_primitive_applications_after_reload() {
    assert_shared_collection_comparison_reloading(SHARED_COLLECTION_SOURCE, false, |source| {
        source
            .replace("Item { value: 1 }", "0.0f32 / 0.0f32")
            .replace("Item { value: 2 }", "0.0f32 / 0.0f32")
    });
}

#[test]
fn content_helper_selects_element_operations_in_the_callers_generation() {
    let source = mixed_generation_collection_source();
    // Element comparison belongs to the helper caller; iteration remains pinned to the view.
    assert_shared_collection_comparison_reloading(&source, false, |source| {
        source.replace(
            "impl PartialEq for Local { fn eq(self, other: Self) -> bool { true } }",
            "impl PartialEq for Local { fn eq(self, other: Self) -> bool { false } }",
        )
    });
}

#[test]
fn shared_collection_comparison_composes_the_new_argument_operation() {
    let source = mixed_generation_collection_source();
    assert_shared_collection_comparison_reloading(&source, false, |source| {
        source.replace("bool { true }", "bool { false }")
    });
    let same_nominal = source
        .replace("Item { value: 1 }", "Local {}")
        .replace("Item { value: 2 }", "Local {}");
    assert_shared_collection_comparison_reloading(&same_nominal, false, |source| {
        source.replace("bool { true }", "bool { false }")
    });
}

#[test]
fn shared_tuple_adapter_preserves_each_arguments_generation() {
    let source = r#"
use std::collections::list_content_equal;
fn same<T: PartialEq>(left: T, right: T) -> bool { left == right }
struct Local {}
impl PartialEq for Local { fn eq(self, other: Self) -> bool { true } }
struct Item { val value: i32 }
impl PartialEq for Item { fn eq(self, other: Self) -> bool { true } }
trait Boxer {
    fn one<T: PartialEq>(self, left: T, right: T) -> bool {
        val a = (Local {}, left);
        val b = (Local {}, right);
        val direct = a == b;
        val forwarded = same(a, b);
        val native = list_content_equal(Vec::from([a]), Vec::from([b]));
        val collection = same(Vec::from([a]), Vec::from([b]));
        if direct != forwarded || direct != native || direct != collection { val zero = 0; 1 / zero; }
        direct
    }
}
struct Factory {}
impl Boxer for Factory {}
trait Probe { fn check(self, factory: Boxer) -> bool; }
struct Checker {}
impl Probe for Checker {
    fn check(self, factory: Boxer) -> bool {
        factory.one(Item { value: 1 }, Item { value: 2 })
    }
}
fn make() -> Boxer { Factory {} }
fn probe() -> Probe { Checker {} }
"#;
    assert_shared_collection_comparison_reloading(source, true, |source| {
        source.replace(
            "impl PartialEq for Local { fn eq(self, other: Self) -> bool { true } }",
            "impl PartialEq for Local { fn eq(self, other: Self) -> bool { false } }",
        )
    });
    assert_shared_collection_comparison_reloading(source, false, |source| {
        source.replace("bool { true }", "bool { false }")
    });
    let same_nominal = source
        .replace("Item { value: 1 }", "Local {}")
        .replace("Item { value: 2 }", "Local {}");
    assert_shared_collection_comparison_reloading(&same_nominal, false, |source| {
        source.replace("bool { true }", "bool { false }")
    });
}

fn mixed_generation_collection_source() -> String {
    SHARED_COLLECTION_SOURCE
        .replace("struct Item", "struct Local {}\nimpl PartialEq for Local { fn eq(self, other: Self) -> bool { true } }\nstruct Item")
        .replace("List<T>", "List<(Local, T)>")
        .replace("Vec::from([value])", "Vec::from([(Local {}, value)])")
}

const SHARED_COLLECTION_SOURCE: &str = r#"
use std::collections::{List, list_content_equal};
struct Item { val value: i32 }
impl PartialEq for Item { fn eq(self, other: Self) -> bool { true } }
fn pack<T>(value: T) -> List<T> { Vec::from([value]) }
trait Boxer {
    fn one<T>(self, value: T) -> List<T> {
        val make = || pack(value);
        make()
    }
}
struct Factory {}
impl Boxer for Factory {}
trait Probe { fn check(self, factory: Boxer) -> bool; }
struct Checker {}
impl Probe for Checker {
    fn check(self, factory: Boxer) -> bool {
        list_content_equal(factory.one(Item { value: 1 }), factory.one(Item { value: 2 }))
    }
}
fn make() -> Boxer { Factory {} }
fn probe() -> Probe { Checker {} }
"#;

fn assert_shared_collection_comparison_reloading(
    source: &str,
    new_expected: bool,
    replacement: impl FnOnce(&str) -> String,
) {
    let engine = KagariEngine::default();
    let original = prepare(&engine, source);
    let replacement = prepare(&engine, &replacement(source));
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = Runtime::new(config);
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    let old = runtime
        .load_program("shared-comparison-reload", original)
        .unwrap();
    let old_key = old.key();
    let vm = Vm::new(runtime);
    let factory = vm.execute(&old, "make").unwrap().return_value;
    let old_probe = vm.execute(&old, "probe").unwrap().return_value;
    let current = vm
        .reload_program(&old, "shared-comparison-reload", replacement)
        .unwrap();
    drop(old);
    vm.runtime().collect_garbage().unwrap();
    let fresh_probe = vm.execute(&current, "probe").unwrap().return_value;
    let contract = current
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Probe")
        .unwrap();
    let method =
        ModuleDecl::method_id(contract.declaration, "check", current.definitions()).unwrap();
    for (probe, expected) in [(&old_probe, true), (&fresh_probe, new_expected)] {
        assert_eq!(
            vm.invoke_interface_method(
                &probe.value(vm.runtime().gc()).unwrap(),
                &method,
                &[factory.value(vm.runtime().gc()).unwrap()],
            )
            .unwrap(),
            Value::Bool(expected),
        );
    }
    drop((factory, old_probe, fresh_probe));
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values,
        0,
    );
}
