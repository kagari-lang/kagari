use super::compile;
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, metadata::DefinitionMetadata,
};
use kagari_runtime::{
    Runtime, module::entry::ProgramEntry, native::module::NativeModule, value::Value,
};
use kagari_vm::vm::Vm;

#[test]
fn rooted_child_and_parent_views_call_current_helpers_across_gc_and_reload() {
    let source = r#"
pub trait Read { fn read(self) -> i32; }
pub trait Child: Read {}
struct Number {}
impl Read for Number { fn read(self) -> i32 { answer() } }
impl Child for Number {}
fn answer() -> i32 { 42 }
pub fn boxed() -> Child { Number {} }
fn main() -> i32 { boxed().read() }
"#;
    let artifact = compile(source).unwrap();
    let mut vm = Vm::new({
        let mut registered = Runtime::default();
        NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut registered).unwrap();
        registered
    });
    let (definitions, artifact) = artifact.into_parts();
    let program =
        DefinitionMetadata::checked(definitions, artifact.program, &Default::default()).unwrap();
    let loaded = vm
        .runtime_mut()
        .load_program("inheritance", program)
        .unwrap();
    let _loaded_program = loaded;
    let loaded = vm.runtime().select_program(&_loaded_program).unwrap();
    let source_root = vm.execute(&loaded, "boxed").unwrap().return_value;
    // SAFETY: The source remains retained until the independent data root is published.
    let value = unsafe { source_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result");
    let root = unsafe { vm.runtime().gc().root_value_transport(value) }.unwrap();
    drop(source_root);
    let method = DefinitionPath {
        module: loaded.bytecode.identity.clone(),
        path: vec![
            DefinitionPathSegment {
                kind: DefinitionKind::Trait,
                name: "Read".into(),
                occurrence: 0,
            },
            DefinitionPathSegment {
                kind: DefinitionKind::Method,
                name: "read".into(),
                occurrence: 0,
            },
        ],
    };
    let method = loaded.definitions().lookup(&method).unwrap();
    let resolved = unsafe {
        vm.runtime()
            .resolve_interface_method_transport(&value, &method)
    }
    .unwrap();
    let parent = resolved.interface_type().clone();
    drop(resolved);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        unsafe { vm.invoke_interface_method_transport(&value, &method, &[]) }.unwrap(),
        Value::I32(42)
    );
    let replacement =
        compile(&source.replace("fn answer() -> i32 { 42 }", "fn answer() -> i32 { 43 }")).unwrap();
    vm.reload_artifact(
        &_loaded_program,
        Some("inheritance".into()),
        replacement,
        &Default::default(),
    )
    .unwrap();
    let _new_program = _loaded_program.clone();
    let new = vm.runtime().select_program(&_new_program).unwrap();
    assert_eq!(
        vm.execute(&new, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(43)
    );
    assert_eq!(
        unsafe { vm.invoke_interface_method_transport(&value, &method, &[]) }.unwrap(),
        Value::I32(43)
    );
    assert_eq!(
        loaded
            .definitions()
            .resolve(parent.declaration)
            .unwrap()
            .segments()
            .last()
            .unwrap()
            .name,
        "Read"
    );
    let foreign = {
        let mut registered = Runtime::default();
        NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut registered).unwrap();
        registered
    };
    assert!(unsafe { foreign.resolve_interface_method_transport(&value, &method) }.is_err());
    drop(root);
    vm.runtime().collect_garbage().unwrap();
    assert!(unsafe { vm.invoke_interface_method_transport(&value, &method, &[]) }.is_err());
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}
