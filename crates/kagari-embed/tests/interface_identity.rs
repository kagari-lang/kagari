#[path = "support/execution.rs"]
mod execution;

use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_embed::{BytecodeArtifact, engine::EngineConfig, error::EmbeddingError};
use kagari_runtime::{Runtime, RuntimeConfig, native::module::NativeModule, value::Value};
use kagari_source::source::SourceFile;
use kagari_types::declaration::module::ModuleDecl;
use kagari_vm::vm::Vm;

#[test]
fn ordinary_interfaces_preserve_underlying_object_identity() {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = execution::engine(config);
    let source = r#"
trait Read { fn read(self) -> i32; }
trait Child: Read {}
struct Item { val value: i32 }
struct Other { val value: i32 }
impl Read for Item { fn read(self) -> i32 { self.value } }
impl Child for Item {}
impl Read for Other { fn read(self) -> i32 { self.value } }
fn boxed(value: Item) -> Child { value }
fn main() -> i32 {
    val item = Item { value: 42 };
    val child: Child = boxed(item);
    val parent: Read = child;
    val again: Read = item;
    val separate: Read = Item { value: 42 };
    val other: Read = Other { value: 42 };
    val joined = if true { child } else { parent };
    if !(item === child && child === parent && parent === again && joined === item) { return 1; }
    if parent !== again || parent === separate || parent === other { return 2; }
    if !(parent !== separate && parent !== other) { return 3; }
    parent.read()
}
"#;
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("interface-identity.kgr", source),
            Default::default(),
        )
        .unwrap();
    execution::assert_returns_i32(
        &engine,
        artifact,
        &[
            execution::Route::Source,
            execution::Route::Artifact,
            execution::Route::ArtifactJit,
        ],
        42,
    );
}

#[test]
fn hidden_value_identity_traps_after_operands_without_leaking_roots() {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = execution::engine(config);
    let source = r#"
trait Read { fn read(self) -> i32; }
trait Child: Read {}
struct State { var trace: i32 }
enum Choice { One }
impl Read for State { fn read(self) -> i32 { self.trace } }
impl Child for State {}
impl Read for i32 { fn read(self) -> i32 { self } }
impl Child for i32 {}
impl Read for (i32, i32) { fn read(self) -> i32 { 7 } }
impl Child for (i32, i32) {}
impl Read for Choice { fn read(self) -> i32 { 7 } }
impl Child for Choice {}
impl Read for String { fn read(self) -> i32 { 7 } }
impl Child for String {}
trait Test {
    fn compare(self, left: i32, right: i32, negated: bool) -> bool;
    fn alias(self, kind: i32) -> bool;
    fn trace(self) -> i32;
}
impl Test for State {
    fn compare(self, left: i32, right: i32, negated: bool) -> bool { compare(self, left, right, negated) }
    fn alias(self, kind: i32) -> bool { alias(self, kind) }
    fn trace(self) -> i32 { self.trace }
}
fn make() -> Test { State { trace: 0 } }
fn text() -> String { "value" }
fn operand(state: State, digit: i32, kind: i32) -> Child {
    state.trace = state.trace * 10 + digit;
    if kind == 0 { return state; }
    if kind == 1 { return 7; }
    if kind == 2 { return (7, 8); }
    if kind == 3 { return Choice::One; }
    text()
}
fn compare(state: State, left: i32, right: i32, negated: bool) -> bool {
    val result = if negated {
        operand(state, 1, left) !== operand(state, 2, right)
    } else {
        operand(state, 1, left) === operand(state, 2, right)
    };
    state.trace = state.trace * 10 + 3;
    result
}
fn alias(state: State, kind: i32) -> bool {
    val child = operand(state, 1, kind);
    val parent: Read = child;
    child === parent
}
"#;
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("interface-identity-traps.kgr", source),
            Default::default(),
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let (definitions, artifact) = artifact.into_parts();
    let program =
        DefinitionMetadata::checked(definitions, artifact.program, &Default::default()).unwrap();
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = Runtime::new(config);
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    let loaded = runtime.load_program("interface-identity", program).unwrap();
    let declaration = loaded
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Test")
        .unwrap()
        .declaration;
    let method = |name| ModuleDecl::method_id(declaration, name, loaded.definitions()).unwrap();
    let vm = Vm::new(runtime);
    // Materialize the lazily cached string constant before observing execution
    // cleanup; this retained program value is not an interface/operand leak.
    drop(vm.execute(&loaded, "text").unwrap());
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    let baseline = vm.runtime().collect_garbage().unwrap().live_objects;
    for kind in 1..=4 {
        for (entry, left, right, negated, expected_trace) in [
            ("compare", kind, 0, false, 12),
            ("compare", 0, kind, false, 12),
            ("compare", kind, kind, true, 12),
            ("alias", kind, kind, false, 1),
        ] {
            let root = vm.execute(&loaded, "make").unwrap().return_value;
            let state = root.value(vm.runtime().gc()).unwrap();
            let arguments = if entry == "alias" {
                vec![Value::I32(kind)]
            } else {
                vec![Value::I32(left), Value::I32(right), Value::Bool(negated)]
            };
            let error = vm
                .invoke_interface_method(&state, &method(entry), &arguments)
                .unwrap_err();
            assert!(
                format!("{error:?}").contains("identity comparison requires object references"),
                "{error:?}"
            );
            assert_eq!(vm.runtime().gc().active_roots(), 1);
            assert!(!vm.runtime().is_quarantined());
            assert_eq!(
                vm.invoke_interface_method(&state, &method("trace"), &[])
                    .unwrap(),
                Value::I32(expected_trace)
            );
            drop(root);
            assert_eq!(vm.runtime().gc().active_roots(), 0);
            assert_eq!(
                vm.runtime().collect_garbage().unwrap().live_objects,
                baseline,
                "{entry}, kind={kind}, left={left}, right={right}"
            );
        }
    }
}

#[test]
fn known_values_and_incompatible_interfaces_reject_identity_operators() {
    let engine = execution::engine(EngineConfig::default());
    for source in [
        "fn main() -> bool { 1 === 1 }",
        "fn main() -> bool { \"x\" !== \"x\" }",
        "fn main() -> bool { (1, 2) === (1, 2) }",
        "enum Choice { One } fn main() -> bool { Choice::One === Choice::One }",
        "trait A {} trait B {} fn compare(a: A, b: B) -> bool { a === b }",
    ] {
        let Err(EmbeddingError::Diagnostics { diagnostics }) = engine.compile_to_artifact(
            SourceFile::new("invalid-identity.kgr", source),
            Default::default(),
        ) else {
            panic!("identity operands must fail source checking: {source}");
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "KG_TYPE_BINARY_OPERAND_TYPE_MISMATCH"),
            "{source}: {diagnostics:?}"
        );
    }
}
