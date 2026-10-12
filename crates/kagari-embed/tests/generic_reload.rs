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
fn interface_comparison_retains_its_receiver_generation_in_a_new_caller() {
    let source = r#"
use std::collections::Set;
use std::iter::CollectionCursor;
struct Bag { val value: i32 }
impl PartialEq for Bag { fn eq(self, other: Self) -> bool { true } }
impl Iterable for Bag {
    type Item = i32;
    type Iter = CollectionCursor<i32>;
    fn iter(self) -> CollectionCursor<i32> { Vec::from([self.value]).iter() }
}

impl Set<i32> for Bag {
    fn len(self) -> usize { 1usize }
    fn is_empty(self) -> bool { false }
    fn contains(self, value: i32) -> bool { self.value == value }
}
trait Probe { fn same(self, a: Set<i32>, b: Set<i32>) -> bool; }
struct Checker {}
impl Probe for Checker { fn same(self, a: Set<i32>, b: Set<i32>) -> bool { a == b } }
fn make() -> Set<i32> { Bag { value: 42 } }
fn probe() -> Probe { Checker {} }
"#;
    let engine = KagariEngine::default();
    let original = prepare(&engine, source);
    let replacement = prepare(&engine, &source.replace("bool { true }", "bool { false }"));
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
fn shared_collection_comparison_composes_old_and_new_nominal_arguments() {
    let source = mixed_generation_collection_source();
    assert_shared_collection_comparison_reloading(&source, true, |source| {
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
fn shared_collection_comparison_preserves_ineligible_content_identity() {
    let source = mixed_generation_collection_source()
        .replace(
            "struct Item { val value: i32 }\nimpl PartialEq for Item { fn eq(self, other: Self) -> bool { true } }",
            "trait Opaque {}\nimpl Opaque for i32 {}\nenum Item { Value(Opaque) }",
        )
        .replace(
            "factory.one(Item { value: 1 }) == factory.one(Item { value: 2 })",
            "val value: Opaque = 1;\n        val a = factory.one(Item::Value(value));\n        val b = factory.one(Item::Value(value));\n        a == a && !(a == b)",
        );
    assert_shared_collection_comparison_reloading(&source, true, |source| {
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

#[test]
fn shared_collection_identity_does_not_require_a_comparison_application() {
    let source = mixed_generation_collection_source().replace(
        "factory.one(Item { value: 1 }) == factory.one(Item { value: 2 })",
        "val a = factory.one(Item { value: 1 });
        val b = factory.one(Item { value: 2 });
        a === a && !(a === b) && !(a !== a) && a !== b",
    );
    assert_shared_collection_comparison_reloading(&source, true, |source| {
        source.replace("bool { true }", "bool { false }")
    });
}

#[test]
fn shared_collection_comparison_retains_a_generic_receiver_selection() {
    let source = generic_bag_collection_source();
    assert_shared_collection_comparison_reloading(&source, true, |source| {
        source.replace("bool { true }", "bool { false }")
    });
    let constrained = source
        .replace(
            "impl<T> PartialEq for Bag<T>",
            "impl<T: Eq> PartialEq for Bag<T>",
        )
        .replace("struct Local", "impl Eq for Local {}\nstruct Local")
        .replace("struct Item", "impl Eq for Item {}\nstruct Item");
    // Eq has no callable. Its positive and negative facts must independently
    // follow the generation of the actual element, not the factory's body.
    assert_shared_collection_comparison_reloading(&constrained, true, |source| {
        source.replace("bool { true }", "bool { false }")
    });
    let negative = format!(
        "struct Unequal {{}}\nimpl PartialEq for Unequal {{ fn eq(self, other: Self) -> bool {{ true }} }}\n{constrained}"
    );
    assert_shared_collection_comparison_reloading(&negative, false, |source| {
        source
            .replace("Item { value: 1 }", "Unequal {}")
            .replace("Item { value: 2 }", "Unequal {}")
    });
}

#[test]
fn shared_collection_comparison_preserves_generic_identity_default() {
    let source = generic_bag_collection_source()
        .replace(
            "impl<T> PartialEq for Bag<T> { fn eq(self, other: Self) -> bool { true } }",
            "",
        )
        .replace(
            "factory.one(Item { value: 1 }) == factory.one(Item { value: 2 })",
            "val a = factory.one(Item { value: 1 });\n        val b = factory.one(Item { value: 2 });\n        a == a && !(a == b)",
        );
    assert_shared_collection_comparison_reloading(&source, true, |source| {
        source.replace("bool { true }", "bool { false }")
    });
}

#[test]
fn shared_collection_comparison_checks_specialized_receiver_arguments() {
    let source = generic_bag_collection_source().replace(
        "impl<T> PartialEq for Bag<T>",
        "impl PartialEq for Bag<(Local, Item)>",
    ).replace(
        "factory.one(Item { value: 1 }) == factory.one(Item { value: 2 })",
        "val a = factory.one(Item { value: 1 });\n        val b = factory.one(Item { value: 2 });\n        if !(a == a) { val zero = 0; 1 / zero; }\n        a == b",
    );
    assert_shared_collection_comparison_reloading(&source, false, |source| {
        source.replace("bool { true }", "bool { false }")
    });
    let generic = generic_bag_collection_source()
        .replace(
            "impl<T> PartialEq for Bag<T>",
            "impl<T: Eq> PartialEq for Bag<(T, i32)>",
        )
        .replace("List<(Local, T)>", "List<T>")
        .replace("Vec::from([(Local {}, value)])", "Vec::from([value])")
        .replace("struct Item", "impl Eq for Item {}\nstruct Item")
        .replace("Item { value: 1 }", "(Item { value: 1 }, 1)")
        .replace("Item { value: 2 }", "(Item { value: 2 }, 1)");
    assert_shared_collection_comparison_reloading(&generic, true, |source| {
        source.replace("bool { true }", "bool { false }")
    });
}

fn generic_bag_collection_source() -> String {
    let source = mixed_generation_collection_source().replace(
        "Vec::from([(Local {}, value)])",
        "Bag { items: Vec::from([(Local {}, value)]) }",
    );
    format!(
        r#"
use std::ops::Index;
use std::iter::CollectionCursor;
struct Bag<T> {{ val items: Vec<T> }}
impl<T> PartialEq for Bag<T> {{ fn eq(self, other: Self) -> bool {{ true }} }}
impl<T> Index<usize> for Bag<T> {{
    type Output = T;
    fn index(self, index: usize) -> T {{ self.items[index] }}
}}
impl<T> Iterable for Bag<T> {{
    type Item = T;
    type Iter = CollectionCursor<T>;
    fn iter(self) -> CollectionCursor<T> {{ self.items.iter() }}
}}
impl<T> List<T> for Bag<T> {{
    fn len(self) -> usize {{ self.items.len() }}
    fn is_empty(self) -> bool {{ self.items.is_empty() }}
    fn get(self, index: usize) -> Option<T> {{ self.items.get(index) }}
}}
{source}
"#,
    )
}

fn mixed_generation_collection_source() -> String {
    SHARED_COLLECTION_SOURCE
        .replace("struct Item", "struct Local {}\nimpl PartialEq for Local { fn eq(self, other: Self) -> bool { true } }\nstruct Item")
        .replace("List<T>", "List<(Local, T)>")
        .replace("Vec::from([value])", "Vec::from([(Local {}, value)])")
}

const SHARED_COLLECTION_SOURCE: &str = r#"
use std::collections::List;
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
        factory.one(Item { value: 1 }) == factory.one(Item { value: 2 })
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
