use crate::{BytecodeArtifact, engine::EngineConfig, test_support::execution};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{Runtime, RuntimeConfig, native::module::NativeModule, value::Value};
use kagari_source::source::SourceFile;
use kagari_types::declaration::module::ModuleDecl;
use kagari_vm::vm::Vm;

#[test]
fn identity_operand_traps_preserve_effect_order_and_release_interface_roots() {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = execution::engine(config);
    let source = r#"
trait Read { fn read(self) -> i32; }
trait Child: Read {}
struct State { var trace: i32 }
impl Read for State { fn read(self) -> i32 { self.trace } }
impl Child for State {}
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
pub fn make() -> Test { State { trace: 0 } }
fn operand(state: State, digit: i32, kind: i32) -> State {
    state.trace = state.trace * 10 + digit;
    val child: Child = state;
    val parent: Read = child;
    parent.read();
    if kind != 0 { val zero = 0; 1 / zero; }
    state
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
    child === state
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
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let declaration = loaded
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Test")
        .unwrap()
        .declaration;
    let method = |name| ModuleDecl::method_id(declaration, name, loaded.definitions()).unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    let baseline = vm.runtime().collect_garbage().unwrap().live_objects;
    for kind in 1..=1 {
        for (entry, left, right, negated, expected_trace) in [
            ("compare", kind, 0, false, 1),
            ("compare", 0, kind, false, 12),
            ("compare", kind, kind, true, 1),
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
                format!("{error:?}").contains("division by zero"),
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
