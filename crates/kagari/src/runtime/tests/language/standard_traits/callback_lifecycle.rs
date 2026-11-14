//! Hash/equality callback guards, reentry, traps and cancellation share one lifecycle.

/* stdlib test support paused (NC01-NC06).
/// The returned interface owns the controller/container graph; no raw tuple child escapes.
fn controller(runtime: &mut Runtime, owner: &ExecutableModule, result: RootedValue) -> RootedValue {
    let mut cx = ConversionContext::new(runtime, owner).unwrap();
    let tuple = cx.import(&result).unwrap();
    assert_eq!(cx.tuple_len(&tuple).unwrap(), 2);
    let controller = cx.tuple_element(&tuple, 0).unwrap();
    cx.retain(&controller).unwrap()
} */

/* Temporarily paused: stdlib restoration (NC01-NC06).
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
trait Test {fn mode(self,value:i32);fn attempt(self);fn calls(self)->i32;fn clear(self);fn len(self)->u64;}
struct Tester {val set:HashSet<Key>,val key:Key,val state:State}
impl Test for Tester {
 fn mode(self,value:i32){self.state.mode=value;}
 fn attempt(self){self.set.insert(self.key);}
 fn calls(self)->i32 {self.state.calls}
 fn clear(self){self.set.clear();}
 fn len(self)->u64{self.set.len()}
}
pub fn make()->(Test,HashSet<Key>) {
 val set:HashSet<Key> = HashSet::new();val state=State{mode:0,calls:0};
 set.insert(Key{id:1,owner:set,state:state});
 val tester:Test=Tester{set:set,key:Key{id:2,owner:set,state:state},state:state};
 (tester,set)
}
"#;
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
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
        NativeModule::install_all(&stdlib::modules().unwrap(), &mut registered).unwrap();
        registered
    };
    let (definitions, artifact) = artifact.into_parts();
    let program =
        DefinitionMetadata::checked(definitions, artifact.program, &Default::default()).unwrap();
    let loaded = load_bytecode(&mut runtime, "callback-cleanup", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let declaration = loaded.definitions().lookup(&query).unwrap();
    let method = |name: &str| {
        loaded
            .definitions()
            .lookup_child(declaration, DefinitionKind::Method, name, 0)
            .unwrap()
    };
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let roots = vm.gc_stats().root_registrations;
    for mode in 1..=4 {
        let value = vm
            .execute_module(&loaded, "make", vm.execution_options())
            .unwrap()
            .return_value;
        let root = controller(&mut vm, &loaded, value);
        let _ = vm
            .invoke_interface_method(&root, &method("mode"), &[RootedValue::from(mode)])
            .unwrap();
        let error = vm
            .invoke_interface_method(&root, &method("attempt"), &[])
            .unwrap_err();
        let message = format!("{error:?}");
        assert!(
            message.contains(if mode == 1 || mode == 3 {
                "container mutation during"
            } else if mode == 2 {
                "integer overflow"
            } else {
                "division by zero"
            }),
            "{message}"
        );
        assert_eq!(
            vm.invoke_interface_method(&root, &method("len"), &[])
                .unwrap()
                .as_u64()
                .unwrap(),
            1
        );
        vm.collect_garbage().unwrap();
        assert_eq!(vm.gc_stats().root_registrations, roots + 1);
        if mode <= 2 {
            assert_eq!(
                vm.invoke_interface_method(&root, &method("calls"), &[])
                    .unwrap()
                    .as_i32()
                    .unwrap(),
                1
            );
        }
        let _ = vm
            .invoke_interface_method(&root, &method("clear"), &[])
            .unwrap();
        assert_eq!(
            vm.invoke_interface_method(&root, &method("len"), &[])
                .unwrap()
                .as_u64()
                .unwrap(),
            0
        );
        assert!(!vm.is_quarantined());
        drop(root);
    }

    // Nested reads use a second guard; dropping it must preserve the outer guard.
    let value = vm
        .execute_module(&loaded, "make", vm.execution_options())
        .unwrap()
        .return_value;
    let root = controller(&mut vm, &loaded, value);
    let _ = vm
        .invoke_interface_method(&root, &method("mode"), &[RootedValue::from(5_i32)])
        .unwrap();
    let _ = vm
        .invoke_interface_method(&root, &method("attempt"), &[])
        .unwrap();
    assert_eq!(
        vm.invoke_interface_method(&root, &method("len"), &[])
            .unwrap()
            .as_u64()
            .unwrap(),
        2
    );
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().root_registrations, roots + 1);
    drop(root);

    for limit in [2, 12, 24, 40] {
        let value = vm
            .execute_module(&loaded, "make", vm.execution_options())
            .unwrap()
            .return_value;
        let root = controller(&mut vm, &loaded, value);
        let _options = vm.execution_options();

        let mut session = support::cancel_after(&mut vm, &loaded, limit);
        let result = session
            .runtime_mut()
            .invoke_interface_method(&root, &method("attempt"), &[]);
        drop(session);
        if result.is_err() {
            assert_eq!(
                vm.invoke_interface_method(&root, &method("len"), &[])
                    .unwrap()
                    .as_u64()
                    .unwrap(),
                1
            );
        }
        vm.collect_garbage().unwrap();
        assert_eq!(vm.gc_stats().root_registrations, roots + 1);
        let _ = vm
            .invoke_interface_method(&root, &method("clear"), &[])
            .unwrap();
        assert_eq!(
            vm.invoke_interface_method(&root, &method("len"), &[])
                .unwrap()
                .as_u64()
                .unwrap(),
            0
        );
        drop(root);
    }
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().root_registrations, roots);
}
*/
