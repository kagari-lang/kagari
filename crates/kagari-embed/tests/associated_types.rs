use kagari_common::{
    SourceFile,
    identity::{ModuleIdentity, PackageId},
    source_database::SourceLayer,
};
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};
use kagari_runtime::value::Value;

fn execute_artifact(engine: &KagariEngine, artifact: BytecodeArtifact) {
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

fn execute(source: &str) {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("associated.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    execute_artifact(&engine, artifact);
}

#[test]
fn generic_implementations_normalize_nested_associated_types() {
    execute(
        r#"
        trait Reader { type Item; fn read(self) -> Self::Item; }
        struct Holder<T> { val value: T }
        impl<T> Reader for Holder<T> {
            type Item = T;
            fn read(self) -> Self::Item { val result: Self::Item = self.value; result }
        }
        fn read<R: Reader>(r: R) -> R::Item { r.read() }
        fn main() -> i32 { read(Holder<i32> { value: 42 }) }
    "#,
    );
    execute(
        r#"
        trait Reader { type Item; fn read(self) -> Self::Item; }
        struct Number { val value: i32 }
        impl Reader for Number { type Item = [i32]; fn read(self) -> Self::Item { [self.value] } }
        fn read(r: Reader<Item = [i32]>) -> [i32] { r.read() }
        fn main() -> i32 { read(Number { value: 42 })[0] }
    "#,
    );
}

#[test]
fn associated_and_where_bounds_support_static_method_calls() {
    for bound in ["type Item: Describe;", "type Item;"] {
        let where_clause = if bound == "type Item;" {
            "where R::Item: Describe"
        } else {
            ""
        };
        execute(&format!(
            r#"
            trait Describe {{ fn number(self) -> i32; }}
            struct Number {{ val value: i32 }}
            impl Describe for Number {{ fn number(self) -> i32 {{ self.value }} }}
            trait Reader {{ {bound} fn read(self) -> Self::Item; }}
            struct Source {{ val value: Number }}
            impl Reader for Source {{ type Item = Number; fn read(self) -> Self::Item {{ self.value }} }}
            fn read<R: Reader>(r: R) -> i32 {where_clause} {{ r.read().number() }}
            fn main() -> i32 {{ read(Source {{ value: Number {{ value: 42 }} }}) }}
        "#
        ));
    }
}

#[test]
fn qualified_projection_disambiguates_same_named_members() {
    execute(
        r#"
        trait Left { type Item; fn left(self) -> Self::Item; }
        trait Right { type Item; fn right(self) -> Self::Item; }
        struct Number { val value: i32 }
        impl Left for Number { type Item = i32; fn left(self) -> Self::Item { self.value } }
        impl Right for Number { type Item = String; fn right(self) -> Self::Item { "right" } }
        fn left<R: Left + Right>(r: R) -> <R as Left>::Item { r.left() }
        fn main() -> i32 { left(Number { value: 42 }) }
    "#,
    );
}

#[test]
fn invalid_associated_type_contracts_are_rejected_before_execution() {
    let engine = KagariEngine::default();
    for source in [
        "trait Read { type Item; } struct N {} impl Read for N {}",
        "trait Read { type Item; type Item; }",
        "trait Read { type Item = i32; }",
        "trait Read { type Item; } struct N {} impl Read for N { type Item; }",
        "trait Read { type Item; } struct N {} impl Read for N { type Item = i32; type Item = i32; }",
        "trait Read { type Item; } struct N {} impl Read for N { type Other = i32; }",
        "struct N {} impl N { type Item = i32; }",
        "struct Box<T> { val x: T } fn bad() { val x = Box<Item = i32> { x: 0 }; }",
        "trait Read { type Item; } struct N {} fn bad() -> <N as Read>::Item { 0 }",
        "trait Bound {} trait Read { type Item; } struct N<T> { val value: T } impl<T: Bound> Read for N<T> { type Item = i32; } fn bad() -> <N<i32> as Read>::Item { 0 }",
        "trait Read { type Item; } struct N {} impl Read for N { type Item = Self::Item; }",
        "trait Read { type Item; fn read(self) -> Self::Item; } fn bad(r: Read) {}",
        "trait Read { type Item; } fn bad(r: Read<Other = i32>) {}",
        "trait Read { type Item; } fn bad(r: Read<Item = i32, Item = String>) {}",
        "trait Read<T> { type Item; } fn bad(r: Read<Item = i32, i32>) {}",
        "trait Read { type Item; } fn bad<R: Read>(r: R) -> R::Missing { 0 }",
        "trait Read { type Item; } fn bad<R: Read>(r: R) -> <R as Read<Item = i32>>::Item { 0 }",
        "trait Left { type Item; } trait Right { type Item; } fn bad<R: Left + Right>(r: R) -> R::Item { 0 }",
        "trait Read { type Item; fn read(self) -> Self::Item; } struct N {} impl Read for N { type Item = i32; fn read(self) -> String { \"wrong\" } }",
        "trait Read { type Item; } struct N {} impl Read for N { type Item = i32; } impl Read for N { type Item = String; }",
        "trait Bound {} trait Read { type Item: Bound; } struct N {} impl Read for N { type Item = i32; }",
        "trait Bound {} trait Read { type Item: Bound; } fn bad(r: Read<Item = i32>) {}",
        "trait Read { type Item; } fn bad(r: Read<Item = String>) { val other: Read<Item = i32> = r; }",
        "trait Read { type Item; } struct N {} impl Read for N { type Item = String; } fn require<R: Read<Item = i32>>(r: R) {} fn bad() { require(N {}); }",
        "trait Bound {} trait Read { type Item; fn read(self) -> Self::Item; } struct N {} impl Read for N { type Item = i32; fn read(self) -> i32 { 42 } } fn require<R: Read>(r: R) where R::Item: Bound {} fn bad() { require(N {}); }",
    ] {
        let source = format!("{source} fn main() -> i32 {{ 42 }}");
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-associated.kgr", &source),
                    Default::default(),
                    Default::default()
                )
                .is_err(),
            "accepted invalid contract: {source}"
        );
    }
}

#[test]
fn imported_associated_types_keep_trait_identity_and_interface_bindings() {
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "model",
            "pub trait Reader { type Item; fn read(self) -> Self::Item; } pub struct Number { pub val value: i32 } impl Reader for Number { type Item = i32; fn read(self) -> Self::Item { self.value } } pub fn make() -> Number { Number { value: 42 } }",
        ),
        (
            "root",
            "use pkg::model::{Reader, Number, make}; pub struct Output { pub val item: <Number as Reader>::Item } pub fn concrete() -> <Number as Reader>::Item { val output: <Number as Reader>::Item = 0; output } fn read<R: Reader>(r: R) -> R::Item { r.read() } fn read_interface(r: Reader<Item = i32>) -> i32 { r.read() } fn main() -> i32 { read_interface(make()) + read(make()) - 42 + Output { item: concrete() }.item }",
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
    let root = root.expect("root");
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            root,
            Default::default(),
            &Default::default(),
        )
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    execute_artifact(&engine, artifact);
}

#[test]
fn trait_inputs_and_associated_outputs_remain_distinct() {
    execute(
        r#"
        trait Reader<T> { type Item; fn read(self, input: T) -> Self::Item; }
        struct N {}
        impl Reader<i32> for N { type Item = i32; fn read(self, input: i32) -> Self::Item { input } }
        fn read<R: Reader<i32, Item = i32>>(r: R) -> i32 { r.read(42) }
        fn main() -> i32 { read(N {}) }
    "#,
    );
    execute(
        r#"
        trait Describe { fn number(self) -> i32; }
        struct N { val value: i32 }
        impl Describe for N { fn number(self) -> i32 { self.value } }
        trait Reader { type Item: Describe; fn read(self) -> Self::Item; }
        struct Holder<T> { val value: T }
        impl<T: Describe> Reader for Holder<T> { type Item = T; fn read(self) -> Self::Item { self.value } }
        fn read<R: Reader>(r: R) -> i32 { r.read().number() }
        fn main() -> i32 { read(Holder { value: N { value: 42 } }) }
    "#,
    );
}

#[test]
fn tampered_associated_schemas_and_bounds_are_rejected() {
    use kagari_hir::builtin::surface::StandardTypeConstraint;
    use kagari_ir::module::{
        PublicAbiItem,
        abi::{AbiType, ConstraintAbi},
    };
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("associated-wire.kgr", "pub trait Read { type Item: HashKey; } struct N {} impl Read for N { type Item = i32; } fn main() -> i32 { 42 }"), Default::default(), Default::default()).unwrap();
    for mutation in 0..3 {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        for item in &mut module.public_items {
            match item {
                PublicAbiItem::InterfaceTable(table) => {
                    let AbiType::Trait(instance) = &mut table.trait_type else {
                        panic!("trait instance");
                    };
                    match mutation {
                        0 => {
                            *instance.associated_types.values_mut().next().unwrap() =
                                AbiType::Builtin(kagari_hir::types::BuiltinType::F32);
                        }
                        1 => {
                            instance.associated_types.clear();
                        }
                        _ => {}
                    }
                }
                PublicAbiItem::Trait(contract) if mutation == 2 => {
                    contract.associated_types[0].bounds = vec![
                        ConstraintAbi::Standard(StandardTypeConstraint::HashKey),
                        ConstraintAbi::Standard(StandardTypeConstraint::HashKey),
                    ];
                }
                _ => {}
            }
        }
        assert!(
            kagari_ir::bytecode::verify_program(&program).is_err(),
            "accepted mutation {mutation}"
        );
        assert!(BytecodeArtifact::from_program(program, Default::default()).is_err());
    }
}
