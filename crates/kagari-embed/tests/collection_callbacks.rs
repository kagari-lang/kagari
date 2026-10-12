mod support;

use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_embed::{BytecodeArtifact, engine::KagariEngine};
use kagari_runtime::{Runtime, RuntimeConfig, native::module::NativeModule, value::Value};
use kagari_source::source::SourceFile;
use kagari_types::declaration::module::ModuleDecl;
use kagari_vm::vm::Vm;

#[test]
fn content_callbacks_preserve_effects_and_release_roots_on_all_exits() {
    let source = r#"
use std::collections::{List, list_content_equal};
use std::hash::Hash;
struct State { var mode: i32, var calls: i32 }
struct Item { val owner: Vec<Item>, val state: State }
impl PartialEq for Item {
    fn eq(self, other: Self) -> bool {
        self.state.calls += 1;
        if self.state.mode == 1 { self.owner.clear(); }
        if self.state.mode == 2 { self.owner.clear(); return false; }
        if self.state.mode == 3 { val zero = 0; 1 / zero; }
        if self.state.mode == 4 { return self.owner == other.owner; }
        true
    }
}
impl Eq for Item {}
impl Hash for Item {
    fn hash(self) -> i64 {
        self.state.calls += 1;
        if self.state.mode == 5 { self.owner.clear(); }
        if self.state.mode == 6 { return self.owner.hash(); }
        42i64
    }
}
trait Test {
    fn mode(self, value: i32);
    fn compare(self, helper: bool) -> bool;
    fn hash_items(self) -> i64;
    fn calls(self) -> i32;
    fn size(self) -> usize;
}
struct Tester { val left: Vec<Item>, val right: Vec<Item>, val state: State }
impl Test for Tester {
    fn mode(self, value: i32) { self.state.mode = value; }
    fn compare(self, helper: bool) -> bool {
        if helper {
            val a: List<Item> = self.left;
            val b: List<Item> = self.right;
            list_content_equal(a, b)
        } else { self.left == self.right }
    }
    fn hash_items(self) -> i64 { self.left.hash() }
    fn calls(self) -> i32 { self.state.calls }
    fn size(self) -> usize { self.left.len() }
}
fn make() -> Test {
    val state = State { mode: 0, calls: 0 };
    val left: Vec<Item> = Vec::new();
    val right: Vec<Item> = Vec::new();
    left.push(Item { owner: left, state: state });
    right.push(Item { owner: right, state: state });
    Tester { left: left, right: right, state: state }
}
"#;
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("collection-callbacks.kgr", source),
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
    config.limits.max_call_depth = Some(32);
    let mut runtime = Runtime::new(config);
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    let loaded = runtime
        .load_program("collection-callbacks", program)
        .unwrap();
    let declaration = loaded
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Test")
        .unwrap()
        .declaration;
    let method = |name| ModuleDecl::method_id(declaration, name, loaded.definitions()).unwrap();
    let vm = Vm::new(runtime);
    for helper in [false, true] {
        for mode in 0..=if helper { 4 } else { 6 } {
            let root = vm.execute(&loaded, "make").unwrap().return_value;
            let value = root.value(vm.runtime().gc()).unwrap();
            vm.invoke_interface_method(&value, &method("mode"), &[Value::I32(mode)])
                .unwrap();
            let result = if mode >= 5 {
                vm.invoke_interface_method(&value, &method("hash_items"), &[])
            } else {
                vm.invoke_interface_method(&value, &method("compare"), &[Value::Bool(helper)])
            };
            match mode {
                0 => assert_eq!(result.unwrap(), Value::Bool(true)),
                2 => assert_eq!(result.unwrap(), Value::Bool(false)),
                _ => {
                    let expected = match mode {
                        1 | 5 => "structurally modified iterator",
                        3 => "division by zero",
                        4 | 6 => "call depth",
                        _ => unreachable!(),
                    };
                    let error = format!("{:?}", result.unwrap_err());
                    assert!(
                        error.contains(expected),
                        "helper={helper}, mode={mode}: {error}"
                    );
                }
            }
            assert_eq!(vm.runtime().gc().active_roots(), 1);
            assert!(!vm.runtime().is_quarantined());
            let calls = vm
                .invoke_interface_method(&value, &method("calls"), &[])
                .unwrap();
            if mode == 4 || mode == 6 {
                assert!(matches!(calls, Value::I32(count) if count > 1));
            } else {
                assert_eq!(calls, Value::I32(1));
            }
            assert_eq!(
                vm.invoke_interface_method(&value, &method("size"), &[])
                    .unwrap(),
                Value::U64(if matches!(mode, 1 | 2 | 5) { 0 } else { 1 }),
            );
            drop(root);
            assert_eq!(vm.runtime().gc().active_roots(), 0);
            assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
        }
    }
    for limit in [1, 16, 32] {
        let root = vm.execute(&loaded, "make").unwrap().return_value;
        let value = root.value(vm.runtime().gc()).unwrap();
        vm.invoke_interface_method(&value, &method("mode"), &[Value::I32(4)])
            .unwrap();
        let session = support::cancel_after(vm.runtime(), &loaded, limit);
        let error = vm
            .invoke_interface_method(&value, &method("compare"), &[Value::Bool(true)])
            .unwrap_err();
        drop(session);
        assert!(format!("{error:?}").contains("cancel"), "{error:?}");
        assert_eq!(vm.runtime().gc().active_roots(), 1);
        assert!(!vm.runtime().is_quarantined());
        vm.invoke_interface_method(&value, &method("mode"), &[Value::I32(0)])
            .unwrap();
        assert_eq!(
            vm.invoke_interface_method(&value, &method("compare"), &[Value::Bool(true)])
                .unwrap(),
            Value::Bool(true)
        );
        drop(root);
        assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    }
}
