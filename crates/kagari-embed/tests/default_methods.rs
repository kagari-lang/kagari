use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};
use kagari_runtime::value::Value;

fn execute(source: &str) {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("defaults.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
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
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let result = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value, Value::I32(42));
    }
}

#[test]
fn default_body_calls_the_implementation_override_through_static_and_dynamic_dispatch() {
    execute(
        r#"
trait Read { fn read(self) -> i32; fn twice(self) -> i32 { helper(self.read()) } }
fn helper(x: i32) -> i32 { x * 2 }
struct Number {}
impl Read for Number { fn read(self) -> i32 { 7 } }
fn generic<T: Read>(value: T) -> i32 { value.twice() }
fn main() -> i32 { val dynamic: Read = Number {}; Number {}.twice() + generic(Number {}) + dynamic.twice() }
"#,
    );
}

#[test]
fn explicit_override_takes_precedence_over_the_default_body() {
    execute(
        r#"
trait Read { fn read(self) -> i32 { 0 } }
struct Number {}
impl Read for Number { fn read(self) -> i32 { 21 } }
fn main() -> i32 { val dynamic: Read = Number {}; Number {}.read() + dynamic.read() }
"#,
    );
}

#[test]
fn generic_implementations_and_default_local_binders_are_specialized() {
    execute(
        r#"
trait Read<T> {
    fn read(self) -> T;
    fn again(self) -> T { self.read() }
}
struct Holder<T> { val value: T }
impl<T> Read<T> for Holder<T> { fn read(self) -> T { self.value } }
fn boxed<T>(x: Holder<T>) -> Read<T> { x }
fn main() -> i32 { boxed(Holder { value: 20 }).again() + Holder { value: 22 }.again() }
"#,
    );
    execute(
        r#"
trait Identity { fn identity<T>(self, value: T) -> T { value } fn same(self) -> Self { self } }
struct Number { val value: i32 }
impl Identity for Number {}
fn generic<T: Identity>(x: T) -> i32 { x.identity(42) }
fn main() -> i32 { val n = Number { value: 42 }.same(); generic(n) }
"#,
    );
}

#[test]
fn defaults_can_call_inherited_methods_and_use_associated_outputs() {
    execute(
        r#"
trait Read { type Item; fn read(self) -> Self::Item; fn again(self) -> Self::Item { self.read() } }
trait Child: Read<Item = i32> { fn number(self) -> i32 { self.again() } }
struct Number {}
impl Read for Number { type Item = i32; fn read(self) -> i32 { 21 } }
impl Child for Number {}
fn main() -> i32 { val child: Child = Number {}; child.number() + child.again() }
"#,
    );
}

#[test]
fn imported_defaults_preserve_private_helpers_and_definition_context() {
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::SourceLayer,
    };
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "model",
            "pub trait Read<T> { fn read(self) -> T; fn again(self) -> T { helper(self.read()) } } fn helper<T>(x: T) -> T { x }",
        ),
        (
            "root",
            "use pkg::model::Read; struct Holder<T> { val value: T } impl<T> Read<T> for Holder<T> { fn read(self) -> T { self.value } } fn helper(x: i32) -> i32 { 0 } fn boxed<T>(x: Holder<T>) -> Read<T> { x } fn main() -> i32 { boxed(Holder { value: 42 }).again() }",
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
    let module = &artifact.program.modules[artifact.program.root.index()];
    let default = module
        .functions
        .iter()
        .find(|function| {
            function
                .identity
                .as_ref()
                .is_some_and(|identity| identity.declaration.path.last().unwrap().name == "again")
        })
        .unwrap();
    let origin = default.metadata.debug.source_module.unwrap();
    assert_eq!(
        artifact.program.modules[origin.index()].identity.path,
        ["model"]
    );
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn invalid_default_bodies_are_checked_even_without_an_implementation() {
    let engine = KagariEngine::default();
    for source in [
        "trait Bad { fn bad(self) -> i32 { missing } } fn main() {}",
        "trait Bad { fn bad(self) -> i32 { true } } fn main() {}",
        "trait Bad { fn required(self) -> i32; } struct Number {} impl Bad for Number {} fn main() {}",
        "trait Bad { fn bad(self) -> Self { 0 } } fn main() {}",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-default.kgr", source),
                    Default::default(),
                    Default::default()
                )
                .is_err(),
            "accepted {source}"
        );
    }
}

#[test]
fn default_closures_and_bound_helpers_substitute_the_concrete_self_receiver() {
    execute(
        r#"
trait Read {
    fn read(self) -> i32;
    fn again(self) -> i32 { val call = || helper(self); call() }
}
fn helper<T: Read>(value: T) -> i32 { value.read() }
struct Number {}
impl Read for Number { fn read(self) -> i32 { 42 } }
fn main() -> i32 { Number {}.again() }
"#,
    );
}

#[test]
fn malformed_default_contracts_and_source_origins_are_rejected() {
    let artifact = KagariEngine::default().compile_to_artifact(SourceFile::new("defaults.kgr", "pub trait Read { fn read(self) -> i32 { 42 } } struct Number {} impl Read for Number {} fn main() -> i32 { Number {}.read() }"), Default::default(), Default::default()).unwrap();
    for mutation in 0..3 {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        if mutation < 2 {
            let contract = module
                .public_items
                .iter_mut()
                .find_map(|item| {
                    let kagari_ir::module::PublicAbiItem::Trait(contract) = item else {
                        return None;
                    };
                    Some(contract)
                })
                .unwrap();
            if mutation == 0 {
                contract.default_methods.push(0);
            } else {
                contract.default_methods.push(usize::MAX);
            }
        } else {
            module.functions[0].metadata.debug.source_module =
                Some(kagari_ir::bytecode::ModuleRef::new(999));
        }
        assert!(BytecodeArtifact::from_program(program, Default::default()).is_err());
    }
}
