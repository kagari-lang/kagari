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
        impl Reader for Number { type Item = MutableArray<i32>; fn read(self) -> Self::Item { [self.value] } }
        fn read(r: Reader<Item = MutableArray<i32>>) -> MutableArray<i32> { r.read() }
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
    let artifact = engine.compile_to_artifact(SourceFile::new("associated-wire.kgr", "pub trait Read { type Item: Eq + Hash; } struct N {} impl Read for N { type Item = i32; } fn main() -> i32 { 42 }"), Default::default(), Default::default()).unwrap();
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

#[test]
fn generic_implementation_interface_tables_specialize_and_deduplicate() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "generic-interface.kgr",
                r#"
        trait Reader { type Item; fn read(self) -> Self::Item; }
        struct Holder<T> { val value: T }
        impl<T> Reader for Holder<T> { type Item = T; fn read(self) -> T { self.value } }
        fn boxed<T>(value: Holder<T>) -> Reader<Item = T> { value }
        fn read(value: Reader<Item = i32>) -> i32 { value.read() }
        fn main() -> i32 {
            val first: Reader<Item = i32> = Holder { value: 20 };
            val second: Reader<Item = String> = Holder { value: "text" };
            val third = boxed(Holder { value: 22 });
            if second.read() == "text" { read(first) + third.read() } else { 0 }
        }
    "#,
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let tables = &artifact.program.modules[artifact.program.root.index()].interface_tables;
    assert_eq!(
        tables
            .iter()
            .filter(|table| !table.arguments.is_empty())
            .count(),
        2
    );
    for table in tables.iter().filter(|table| !table.arguments.is_empty()) {
        assert_eq!(table.methods.len(), 1);
    }
    execute_artifact(&engine, artifact);
}

#[test]
fn generic_interface_conversion_checks_implementation_bounds() {
    execute(
        r#"
        trait Reader { type Item; fn read(self) -> Self::Item; }
        struct Holder<T> { val value: T }
        impl<T: Eq + Hash> Reader for Holder<T> { type Item = T; fn read(self) -> T { self.value } }
        fn boxed<T: Eq + Hash>(value: Holder<T>) -> Reader<Item = T> { value }
        fn main() -> i32 { boxed(Holder { value: 42 }).read() }
    "#,
    );
    execute(
        r#"
        trait Describe { fn number(self) -> i32; }
        struct Number { val value: i32 }
        impl Describe for Number { fn number(self) -> i32 { self.value } }
        trait Reader { fn read(self) -> i32; }
        struct Holder<T> { val value: T }
        impl<T: Describe> Reader for Holder<T> { fn read(self) -> i32 { self.value.number() } }
        fn boxed<T: Describe>(value: Holder<T>) -> Reader { value }
        fn main() -> i32 { boxed(Holder { value: Number { value: 42 } }).read() }
    "#,
    );
    let engine = KagariEngine::default();
    for source in [
        "trait Read {} struct Holder<T> { val value: T } impl<T: Eq + Hash> Read for Holder<T> {} fn main() { val reader: Read = Holder { value: 1.5 }; }",
        "trait Read {} struct Holder<T> { val value: T } impl<T: Eq + Hash> Read for Holder<T> {} fn boxed<T>(value: Holder<T>) -> Read { value } fn main() {}",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-generic-interface.kgr", source),
                    Default::default(),
                    Default::default()
                )
                .is_err(),
            "accepted {source}"
        );
    }
}

#[test]
fn interface_instance_bounds_are_checked_without_method_slots() {
    use kagari_hir::types::BuiltinType;
    use kagari_ir::module::abi::AbiType;
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("empty-generic-wire.kgr", "trait Tag {} struct Holder<T> { val value: T } impl<T: Eq + Hash> Tag for Holder<T> {} fn main() -> i32 { val tagged: Tag = Holder { value: 42 }; 42 }"), Default::default(), Default::default()).unwrap();
    let mut program = artifact.program.clone();
    let table = program.modules[program.root.index()]
        .interface_tables
        .iter_mut()
        .find(|table| !table.arguments.is_empty())
        .unwrap();
    assert!(table.methods.is_empty());
    table.arguments[0] = AbiType::Builtin(BuiltinType::F32);
    assert!(kagari_ir::bytecode::verify_program(&program).is_err());
    execute_artifact(&engine, artifact);
}

#[test]
fn imported_generic_interfaces_materialize_all_methods_in_the_owning_module() {
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "model",
            "pub trait Reader<T> { type Item; fn read(self) -> Self::Item; fn add(self, value: T) -> i32; } pub struct Holder<T> { pub val value: T } impl<T> Reader<i32> for Holder<T> { type Item = T; fn add(self, value: i32) -> i32 { value + 2 } fn read(self) -> T { self.value } }",
        ),
        (
            "root",
            "use pkg::model::{Reader, Holder}; fn boxed<T>(value: Holder<T>) -> Reader<i32, Item = T> { value } fn main() -> i32 { val number = boxed(Holder { value: 20 }); val text: Reader<i32, Item = String> = Holder { value: \"text\" }; if text.read() == \"text\" { number.read() + number.add(20) } else { 0 } }",
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
    let model = artifact
        .program
        .modules
        .iter()
        .find(|module| module.identity.path == ["model"])
        .unwrap();
    let tables: Vec<_> = model
        .interface_tables
        .iter()
        .filter(|table| !table.arguments.is_empty())
        .collect();
    assert_eq!(tables.len(), 2);
    assert!(tables.iter().all(|table| table.methods.len() == 2));
    execute_artifact(&engine, artifact);
}

#[test]
fn malformed_generic_interface_instances_are_rejected_before_execution() {
    use kagari_hir::types::BuiltinType;
    use kagari_ir::module::abi::AbiType;
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("generic-wire.kgr", "trait Reader { type Item; fn read(self) -> Self::Item; } struct Holder<T> { val value: T } impl<T: Eq + Hash> Reader for Holder<T> { type Item = T; fn read(self) -> T { self.value } } fn main() -> i32 { val a: Reader<Item = i32> = Holder { value: 42 }; val b: Reader<Item = String> = Holder { value: \"text\" }; a.read() }"), Default::default(), Default::default()).unwrap();
    for mutation in 0..6 {
        let mut program = artifact.program.clone();
        let tables = &mut program.modules[program.root.index()].interface_tables;
        let index = tables
            .iter()
            .position(|table| !table.arguments.is_empty())
            .unwrap();
        match mutation {
            0 => tables[index]
                .arguments
                .push(AbiType::Builtin(BuiltinType::I32)),
            1 => tables[index].methods.clear(),
            2 => {
                let duplicate = tables[index].clone();
                tables.push(duplicate);
            }
            3 => tables[index].methods = tables[index + 1].methods.clone(),
            4 => tables[index].arguments[0] = AbiType::Builtin(BuiltinType::F32),
            5 => {
                tables.remove(index);
            }
            _ => unreachable!(),
        }
        assert!(
            kagari_ir::bytecode::verify_program(&program).is_err(),
            "accepted mutation {mutation}"
        );
        assert!(BytecodeArtifact::from_program(program, Default::default()).is_err());
    }
}
