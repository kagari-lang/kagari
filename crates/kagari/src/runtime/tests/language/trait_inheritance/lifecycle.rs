use super::compile;
use crate::test_support::bytecode::load_bytecode;
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, metadata::DefinitionMetadata,
};
use kagari_runtime::{
    Runtime, error::RuntimeErrorKind, module::entry::ProgramEntry, native::module::NativeModule,
};
use kagari_types::ty::{NominalTy, Ty};

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
    let mut vm = {
        let mut registered = Runtime::default();
        NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut registered).unwrap();
        registered
    };
    let (definitions, artifact) = artifact.into_parts();
    let program =
        DefinitionMetadata::checked(definitions, artifact.program, &Default::default()).unwrap();
    let loaded = load_bytecode(&mut vm, "inheritance", program).unwrap();
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.collect_garbage().unwrap();
    let roots = vm.gc_stats().root_registrations;
    let value = vm
        .execute_module(&loaded, "boxed", vm.execution_options())
        .unwrap()
        .return_value;
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
    let resolved = vm.resolve_interface_method(&value, &method).unwrap();
    let parent = resolved.interface_type().clone();
    let parent_decl = loaded.definitions().parent(method).unwrap().unwrap();
    let expected = vm
        .resolve_type_arguments(
            &loaded,
            &[Ty::Trait(NominalTy {
                declaration: parent_decl,
                arguments: vec![],
                associated_types: Default::default(),
            })],
        )
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(expected.ty(), &Ty::Trait(parent.clone()));
    drop(resolved);
    vm.collect_garbage().unwrap();
    assert_eq!(
        vm.invoke_interface_method(&value, &method, &[])
            .unwrap()
            .as_i32()
            .unwrap(),
        42
    );
    let replacement =
        compile(&source.replace("fn answer() -> i32 { 42 }", "fn answer() -> i32 { 43 }")).unwrap();
    vm.stage_reload_artifact(
        &_loaded_program,
        Some("inheritance".into()),
        replacement,
        &Default::default(),
    )
    .and_then(|candidate| vm.publish_staged_reload(candidate))
    .unwrap();
    let _new_program = _loaded_program.clone();
    let new = vm.select_program(&_new_program).unwrap();
    assert_eq!(
        vm.execute_module(&new, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32()
            .unwrap(),
        43
    );
    assert_eq!(
        vm.invoke_interface_method(&value, &method, &[])
            .unwrap()
            .as_i32()
            .unwrap(),
        43
    );
    assert_eq!(
        expected
            .definitions()
            .resolve(parent.declaration)
            .unwrap()
            .segments()
            .last()
            .unwrap()
            .name,
        "Read"
    );
    let mut foreign = {
        let mut registered = Runtime::default();
        NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut registered).unwrap();
        registered
    };
    assert_eq!(
        foreign
            .resolve_interface_method(&value, &method)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    drop(value);
    vm.collect_garbage().unwrap();
    assert_eq!(
        vm.validate_executable_module(&loaded).unwrap_err().kind(),
        RuntimeErrorKind::StaleHandle
    );
    assert_eq!(vm.gc_stats().root_registrations, roots);
}
