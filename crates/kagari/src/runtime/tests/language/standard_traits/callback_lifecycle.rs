//! Hash/equality callback guards, reentry, traps and cancellation share one lifecycle.
use crate::{BytecodeArtifact, engine::KagariEngine, test_support::cancellation as support};
use kagari_common::identity::{DefinitionKind, metadata::DefinitionMetadata};
use kagari_runtime::{Runtime, RuntimeConfig, native::module::NativeModule, value::Value};
use kagari_source::source::SourceFile;
use kagari_vm::vm::Vm;

#[test]
fn key_callback_traps_and_reentry_release_guards_without_partial_insertion() {
    let source = r#"use std::collections::{HashSet};
use std::hash::{Hash};

struct State {var mode:i32,var calls:i32}
struct Key {val id:i32,val owner:HashSet<Key>,val state:State}
impl PartialEq for Key {fn eq(self,other:Self)->bool {
 self.state.calls+=1;
 if self.state.mode==1 {self.owner.clear();}
 if self.state.mode==2 {val limit = 2147483647; limit + 1;}
 if self.state.mode==5 {
  self.state.mode=0;
  val absent = Key { id:99, owner:self.owner, state:self.state };
  { val passed = !self.owner.contains(absent); if !passed { val zero = 0; 1 / zero; } };
  self.state.mode=5;
 }
 self.id==other.id
}}
impl Eq for Key {}
impl Hash for Key {fn hash(self)->i64 {
 if self.state.mode==3 {self.owner.clear();}
 if self.state.mode==4 {val zero = 0; 1 / zero;}
 0.hash()
}}
trait Test {fn mode(self,value:i32);fn attempt(self);fn calls(self)->i32;fn len(self)->u64;fn clear(self);}
struct Tester {val set:HashSet<Key>,val key:Key,val state:State}
impl Test for Tester {
 fn mode(self,value:i32){self.state.mode=value;}
 fn attempt(self){self.set.insert(self.key);}
 fn calls(self)->i32 {self.state.calls}
 fn len(self)->u64 {self.set.len()}
 fn clear(self){self.set.clear();}
}
pub fn make()->(Test,HashSet<Key>) {
 val set:HashSet<Key> = HashSet::new();val state=State{mode:0,calls:0};
 set.insert(Key{id:1,owner:set,state:state});
 val tester:Test=Tester{set:set,key:Key{id:2,owner:set,state:state},state:state};
 (tester,set)
}
"#;
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("callback-cleanup.kgr", source),
            Default::default(),
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let root_module = &artifact.records().program.modules[artifact.records().program.root.index()];
    let declaration = root_module
        .trait_contracts
        .iter()
        .find(|c| c.abi.name == "Test")
        .unwrap()
        .declaration;
    let query = artifact
        .definitions()
        .resolve(declaration)
        .unwrap()
        .to_path();
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = {
        let mut registered = Runtime::new(config);
        NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut registered).unwrap();
        registered
    };
    let (definitions, artifact) = artifact.into_parts();
    let program =
        DefinitionMetadata::checked(definitions, artifact.program, &Default::default()).unwrap();
    let loaded = runtime.load_program("callback-cleanup", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let declaration = loaded.definitions().lookup(&query).unwrap();
    let method = |name: &str| {
        loaded
            .definitions()
            .lookup_child(declaration, DefinitionKind::Method, name, 0)
            .unwrap()
    };
    let vm = Vm::new(runtime);
    for mode in 1..=4 {
        let value = vm
            .execute(&loaded, "make")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result");
        let root = vm.runtime().root_value(value).unwrap();
        let Value::Tuple(values) = value else {
            panic!("tuple")
        };
        let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
        vm.invoke_interface_method(&values[0], &method("mode"), &[Value::I32(mode)])
            .unwrap();
        let error = vm
            .invoke_interface_method(&values[0], &method("attempt"), &[])
            .unwrap_err();
        let message = format!("{error:?}");
        assert!(
            message.contains(if mode == 1 || mode == 3 {
                "operation protects receiver slots"
            } else if mode == 2 {
                "integer overflow"
            } else {
                "division by zero"
            }),
            "{message}"
        );
        assert!(matches!(values[1], Value::GcHandle(_)));
        assert_eq!(
            vm.invoke_interface_method(&values[0], &method("len"), &[])
                .unwrap(),
            Value::U64(1)
        );
        assert_eq!(vm.runtime().gc().active_roots(), 1);
        if mode <= 2 {
            assert_eq!(
                vm.invoke_interface_method(&values[0], &method("calls"), &[])
                    .unwrap(),
                Value::I32(1)
            );
        }
        vm.invoke_interface_method(&values[0], &method("clear"), &[])
            .unwrap();
        assert_eq!(
            vm.invoke_interface_method(&values[0], &method("len"), &[])
                .unwrap(),
            Value::U64(0)
        );
        assert!(!vm.runtime().is_quarantined());
        drop(root);
    }

    // Nested reads use a second guard; dropping it must preserve the outer guard.
    let value = vm
        .execute(&loaded, "make")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(value).unwrap();
    let Value::Tuple(values) = value else {
        panic!("tuple")
    };
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
    vm.invoke_interface_method(&values[0], &method("mode"), &[Value::I32(5)])
        .unwrap();
    vm.invoke_interface_method(&values[0], &method("attempt"), &[])
        .unwrap();
    assert!(matches!(values[1], Value::GcHandle(_)));
    assert_eq!(
        vm.invoke_interface_method(&values[0], &method("len"), &[])
            .unwrap(),
        Value::U64(2)
    );
    assert_eq!(vm.runtime().gc().active_roots(), 1);
    drop(root);

    for limit in [2, 12, 24, 40] {
        let value = vm
            .execute(&loaded, "make")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result");
        let root = vm.runtime().root_value(value).unwrap();
        let Value::Tuple(values) = value else {
            panic!("tuple")
        };
        let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
        assert!(matches!(values[1], Value::GcHandle(_)));
        let _options = vm.runtime().execution_options();

        let session = support::cancel_after(vm.runtime(), &loaded, limit);
        let result = vm.invoke_interface_method(&values[0], &method("attempt"), &[]);
        drop(session);
        if result.is_err() {
            assert_eq!(
                vm.invoke_interface_method(&values[0], &method("len"), &[])
                    .unwrap(),
                Value::U64(1)
            );
        }
        assert_eq!(vm.runtime().gc().active_roots(), 1);
        vm.invoke_interface_method(&values[0], &method("clear"), &[])
            .unwrap();
        assert_eq!(
            vm.invoke_interface_method(&values[0], &method("len"), &[])
                .unwrap(),
            Value::U64(0)
        );
        drop(root);
    }
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}
