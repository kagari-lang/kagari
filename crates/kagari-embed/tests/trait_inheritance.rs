use kagari_common::SourceFile;
use kagari_embed::{
    ArtifactOptions, BytecodeArtifact, CompileOptions, ExecutionContext, KagariEngine,
};
use kagari_runtime::value::Value;

fn compile(source: &str) -> Result<BytecodeArtifact, kagari_embed::EmbeddingError> {
    KagariEngine::default().compile_to_artifact(
        SourceFile::new("inheritance.kgr", source),
        CompileOptions::default(),
        ArtifactOptions::default(),
    )
}

fn execute(artifact: BytecodeArtifact) {
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let mut context = ExecutionContext::default();
        context.capabilities.jit = jit;
        context.language_profile.allow_jit = jit;
        context.jit_policy = if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = KagariEngine::default().runtime(context.clone());
        let program = runtime.load_program(artifact, Default::default()).unwrap();
        let result = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&program, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&program, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value, Value::I32(42));
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
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::SourceLayer,
    };
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
        .compile_snapshot(
            engine.source_snapshot(),
            root.unwrap(),
            Default::default(),
            &Default::default(),
        )
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
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
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let mut runtime = KagariEngine::default().runtime(context.clone());
        let program = runtime.load_program(artifact, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&program, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
    }
}

#[test]
fn cycles_missing_parents_and_distinct_parent_methods_are_diagnostics() {
    for source in [
        "trait A: A {} fn main() {}",
        "trait A: B {} trait B: A {} fn main() {}",
        "trait A<T>: A<Array<T>> {} fn main() {}",
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
fn boxed(value: Child) -> Read { value }
fn main() -> i32 {
    val child: Child = Number { value: 14 };
    val parent_view: Read = child;
    child.read() + parent(parent_view) + boxed(child).read()
}
"#;
    let artifact = compile(source).unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = KagariEngine::default().runtime(context.clone());
    let program = runtime.load_program(artifact, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&program, "main", &[], &context)
            .unwrap()
            .return_value,
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
fn boxed<T>(value: Holder<T>) -> Child<T> { value }
fn parent(value: Child<i32>) -> Read<i32> { value }
fn main() -> i32 { parent(boxed(Holder { value: 42 })).read() }
"#;
    let artifact = compile(source).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = KagariEngine::default().runtime(context.clone());
    let program = runtime.load_program(artifact, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&program, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn artifact_inheritance_cycles_and_missing_parent_implementations_are_rejected() {
    use kagari_ir::module::PublicAbiItem;
    let artifact = compile("pub trait Parent {} pub trait Child: Parent {} struct S {} impl Parent for S {} impl Child for S {} fn main() {}").unwrap();
    for cycle in [true, false] {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        if cycle {
            let parent = module
                .public_items
                .iter()
                .find_map(|item| match item {
                    PublicAbiItem::Trait(ty) if ty.name == "Child" => {
                        Some(ty.supertraits[0].clone())
                    }
                    _ => None,
                })
                .unwrap();
            let PublicAbiItem::Trait(ty) = module
                .public_items
                .iter_mut()
                .find(|item| matches!(item, PublicAbiItem::Trait(ty) if ty.name == "Parent"))
                .unwrap()
            else {
                unreachable!()
            };
            ty.supertraits.push(parent);
        } else {
            module.public_items.retain(|item| !matches!(item, PublicAbiItem::InterfaceTable(table) if matches!(&table.trait_type, kagari_ir::module::abi::AbiType::Trait(ty) if ty.declaration.path.last().unwrap().name == "Parent")));
        }
        assert!(kagari_ir::bytecode::verify_program(&program).is_err());
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
fn boxed() -> Child { Number {} }
fn main() -> i32 { boxed().read() }
"#;
    let artifact = compile(source).unwrap();
    let mut vm = kagari_vm::Vm::new(kagari_runtime::Runtime::default());
    let loaded = vm
        .runtime_mut()
        .load_program("inheritance", artifact.program)
        .unwrap();
    let value = vm.execute(&loaded, "boxed").unwrap().return_value;
    let root = vm.runtime().root_value(value.clone()).unwrap();
    use kagari_common::identity::{DefinitionId, DefinitionKind, DefinitionPathSegment};
    let method = DefinitionId {
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
    let new = vm
        .reload_artifact(&loaded, "inheritance", replacement, &Default::default())
        .unwrap();
    assert_eq!(
        vm.execute(&new, "main").unwrap().return_value,
        Value::I32(43)
    );
    assert_eq!(
        vm.invoke_interface_method(&value, &method, &[]).unwrap(),
        Value::I32(42)
    );
    assert_eq!(parent.declaration.path.last().unwrap().name, "Read");
    let foreign = kagari_runtime::Runtime::default();
    assert!(foreign.resolve_interface_method(&value, &method).is_err());
    drop(root);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.invoke_interface_method(&value, &method, &[]).is_err());
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}
