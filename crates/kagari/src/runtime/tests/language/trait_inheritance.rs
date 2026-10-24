use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{KagariEngine, source::ArtifactOptions},
    error::EmbeddingError,
    program::PreparedProgram,
};
use kagari_bytecode::program::verify_program;
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
    metadata::DefinitionMetadata,
};
use kagari_contract::types::PublicItem;
use kagari_runtime::module::entry::ProgramEntry;
use kagari_runtime::{Runtime, native::module::NativeModule, value::Value};
use kagari_source::{source::SourceFile, source_database::SourceLayer};
use kagari_types::ty::Ty;
use kagari_vm::vm::Vm;

fn compile(source: &str) -> Result<DefinitionMetadata<BytecodeArtifact>, EmbeddingError> {
    KagariEngine::default().compile_to_artifact(
        SourceFile::new("inheritance.kgr", source),
        ArtifactOptions::default(),
    )
}

fn execute(artifact: DefinitionMetadata<BytecodeArtifact>) {
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = KagariEngine::default().runtime(context.clone());
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let result = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &program,
                    &loaded,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        assert_eq!(
            result
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}

#[test]
fn parent_associated_types_are_available_through_child_bounds_and_interfaces() {
    execute(compile(r#"
trait Read { type Item; fn read(self) -> Self::Item; }
trait Child: Read<Item = i32> { fn extra(self) -> Self::Item; }
struct Number { val value: i32 }
impl Read for Number { type Item = i32; fn read(self) -> Self::Item { self.value } }
impl Child for Number { fn extra(self) -> Self::Item { 0 } }
fn generic<T: Child>(x: T) -> T::Item { x.read() }
fn qualified<T: Child>(x: T) -> <T as Read>::Item { x.read() }
fn parent(x: Child) -> Read<Item = i32> { x }
fn main() -> i32 { val x: Child = Number { value: 14 }; generic(Number { value: 14 }) + qualified(Number { value: 14 }) + parent(x).read() + x.extra() }
"#).unwrap());
}

#[test]
fn imported_child_bounds_keep_hidden_parent_declarations_and_projections() {
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "parent",
            "pub trait Read<T> { type Item; fn read(self) -> Self::Item; }",
        ),
        (
            "model",
            "use pkg::parent::Read; pub trait Child<T>: Read<T, Item = T> {} pub struct Holder<T> { pub val value: T } impl<T> Read<T> for Holder<T> { type Item = T; fn read(self) -> T { self.value } } impl<T> Child<T> for Holder<T> {} pub fn boxed(x: Holder<i32>) -> Child<i32> { x }",
        ),
        (
            "root",
            "use pkg::model::{Child, Holder, boxed}; fn read<T: Child<i32>>(x: T) -> T::Item { x.read() } fn main() -> i32 { read(Holder { value: 20 }) + boxed(Holder { value: 22 }).read() }",
        ),
    ] {
        let path = format!("mem://{name}");
        engine
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let id = engine
            .set_source(&path, source.into(), SourceLayer::Base)
            .unwrap();
        if name == "root" {
            root = Some(id);
        }
    }
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), root.unwrap(), &Default::default())
        .unwrap();
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    execute(artifact);
}

#[test]
fn static_bounds_expose_transitive_parent_methods_and_deduplicate_diamonds() {
    let source = r#"
trait Read<T> { fn read(self) -> T; }
trait Left<T>: Read<T> {}
trait Right<T>: Read<T> {}
trait Child<T>: Left<T> + Right<T> {}
struct Number { val value: i32 }
impl Read<i32> for Number { fn read(self) -> i32 { self.value } }
impl Left<i32> for Number {}
impl Right<i32> for Number {}
impl Child<i32> for Number {}
fn parent<T: Read<i32>>(value: T) -> i32 { value.read() }
fn read<T: Child<i32>>(value: T) -> i32 { parent(value) }
fn direct<T: Child<i32>>(value: T) -> i32 { value.read() }
fn main() -> i32 { read(Number { value: 20 }) + direct(Number { value: 22 }) }
"#;
    let artifact = compile(source).unwrap();
    let context = ExecutionContext::default();
    for encoded in [false, true] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap()
        } else {
            artifact.clone()
        };
        let mut runtime = KagariEngine::default().runtime(context.clone());
        let program_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let program = runtime
            .load_program(&program_program, Default::default())
            .unwrap();
        assert_eq!(
            runtime
                .execute(&program, ProgramEntry::Main, &[], &context)
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}

#[test]
fn cycles_missing_parents_and_distinct_parent_methods_are_diagnostics() {
    for source in [
        "trait A: A {} fn main() {}",
        "trait A: B {} trait B: A {} fn main() {}",
        "use std::collections::{List};\ntrait A<T>: A<List<T>> {} fn main() {}",
        "trait Bound {} trait Parent<T: Bound> {} trait Child<T>: Parent<T> {} fn main() {}",
        "trait Parent<T> { fn read(self) -> T; } trait Child: Parent<Self> {} fn bad(x: Child) {} fn main() {}",
        "trait Parent { type Item; } trait Child: Parent {} fn bad(x: Child) {} fn main() {}",
        "trait Parent { fn clone(self) -> Self; } trait Child: Parent {} fn bad(x: Child) {} fn main() {}",
        "trait Parent {} trait Child: Parent {} struct S {} impl Child for S {} fn main() {}",
        "trait A { fn read(self) -> i32; } trait B { fn read(self) -> i32; } trait C: A + B {} fn read<T: C>(x: T) -> i32 { x.read() } fn main() {}",
    ] {
        assert!(compile(source).is_err(), "accepted {source}");
    }
}

#[test]
fn dynamic_child_values_call_parent_methods_and_convert_to_parent_views() {
    let source = r#"
trait Read { fn read(self) -> i32; }
trait Child: Read { fn extra(self) -> i32; }
struct Number { val value: i32 }
impl Read for Number { fn read(self) -> i32 { self.value } }
impl Child for Number { fn extra(self) -> i32 { 1 } }
fn parent(value: Read) -> i32 { value.read() }
pub fn boxed(value: Child) -> Read { value }
fn main() -> i32 {
    val child: Child = Number { value: 14 };
    val parent_view: Read = child;
    child.read() + parent(parent_view) + boxed(child).read()
}
"#;
    let artifact = compile(source).unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = KagariEngine::default().runtime(context.clone());
    let program_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let program = runtime
        .load_program(&program_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&program, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn generic_parent_tables_are_compiled_before_upcasting() {
    let source = r#"
trait Read<T> { fn read(self) -> T; }
trait Child<T>: Read<T> {}
struct Holder<T> { val value: T }
impl<T> Read<T> for Holder<T> { fn read(self) -> T { self.value } }
impl<T> Child<T> for Holder<T> {}
pub fn boxed<T>(value: Holder<T>) -> Child<T> { value }
fn parent(value: Child<i32>) -> Read<i32> { value }
fn main() -> i32 { parent(boxed(Holder { value: 42 })).read() }
"#;
    let artifact = compile(source).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = KagariEngine::default().runtime(context.clone());
    let program_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let program = runtime
        .load_program(&program_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&program, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn artifact_inheritance_cycles_and_missing_parent_implementations_are_rejected() {
    let artifact = compile("pub trait Parent {} pub trait Child: Parent {} struct S {} impl Parent for S {} impl Child for S {} fn main() {}").unwrap();
    for cycle in [true, false] {
        let mut program = artifact.records().program.clone();
        let module = &mut program.modules[program.root.index()];
        if cycle {
            let parent = module
                .public_items
                .iter()
                .find_map(|item| match item {
                    PublicItem::Trait(ty) if ty.name == "Child" => Some(ty.supertraits[0].clone()),
                    _ => None,
                })
                .unwrap();
            let PublicItem::Trait(ty) = module
                .public_items
                .iter_mut()
                .find(|item| matches!(item, PublicItem::Trait(ty) if ty.name == "Parent"))
                .unwrap()
            else {
                unreachable!()
            };
            ty.supertraits.push(parent);
        } else {
            module.public_items.retain(|item| !matches!(item, PublicItem::InterfaceTable(table) if matches!(&table.trait_type, Ty::Trait(ty) if artifact.definitions().resolve(ty.declaration).unwrap().last().unwrap().name == "Parent")));
        }
        assert!(verify_program(&program, artifact.definitions(), &Default::default()).is_err());
    }
}

#[test]
fn rooted_child_and_parent_views_keep_old_versions_across_gc_and_reload() {
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
    let value = vm
        .execute(&loaded, "boxed")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let root = vm.runtime().root_value(value).unwrap();
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
    let resolved = vm
        .runtime()
        .resolve_interface_method(&value, &method)
        .unwrap();
    let parent = resolved.interface_type().clone();
    drop(resolved);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        vm.invoke_interface_method(&value, &method, &[]).unwrap(),
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
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(43)
    );
    assert_eq!(
        vm.invoke_interface_method(&value, &method, &[]).unwrap(),
        Value::I32(42)
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
    assert!(foreign.resolve_interface_method(&value, &method).is_err());
    drop(root);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.invoke_interface_method(&value, &method, &[]).is_err());
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}
