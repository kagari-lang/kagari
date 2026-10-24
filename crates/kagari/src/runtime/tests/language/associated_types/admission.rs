use crate::{BytecodeArtifact, engine::KagariEngine, test_support::execution};
use kagari_bytecode::program::{identity, verify_program};
use kagari_common::identity::map::DefinitionContext;
use kagari_contract::types::PublicItem;
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn tampered_associated_schemas_and_bounds_are_rejected() {
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("associated-wire.kgr", "use std::hash::{Hash};\npub trait Read { type Item: Eq + Hash; } struct N {} impl Read for N { type Item = i32; } fn main() -> i32 { 42 }"),  Default::default()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact.import_into(&context, &Default::default()).unwrap();
    let table = identity::collect(
        &artifact
            .records()
            .program
            .modules
            .iter()
            .collect::<Vec<_>>(),
        &context,
        &Default::default(),
    )
    .unwrap();
    verify_program(&artifact.records().program, &table, &Default::default()).unwrap();
    for mutation in 0..3 {
        let mut program = artifact.records().program.clone();
        let module = &mut program.modules[program.root.index()];
        for item in &mut module.public_items {
            match item {
                PublicItem::InterfaceTable(table) => {
                    let Ty::Trait(instance) = &mut table.trait_type else {
                        panic!("trait instance");
                    };
                    match mutation {
                        0 => {
                            *instance.associated_types.values_mut().next().unwrap() =
                                Ty::Builtin(BuiltinType::F32);
                        }
                        1 => {
                            instance.associated_types.clear();
                        }
                        _ => {}
                    }
                }
                PublicItem::Trait(contract) if mutation == 2 => {
                    let bounds = &mut contract.associated_types[0].bounds;
                    bounds.push(bounds[0].clone());
                }
                _ => {}
            }
        }
        assert!(
            verify_program(&program, &context.snapshot(), &Default::default()).is_err(),
            "accepted mutation {mutation}"
        );
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err()
        );
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
        )
        .unwrap();
    let tables = &artifact.records().program.modules[artifact.records().program.root.index()]
        .interface_tables;
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
    execution::assert_artifact_returns_i32(&engine, artifact, 42);
}

#[test]
fn interface_instance_bounds_are_checked_without_method_slots() {
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("empty-generic-wire.kgr", "use std::hash::{Hash};\ntrait Tag {} struct Holder<T> { val value: T } impl<T: Eq + Hash> Tag for Holder<T> {} fn main() -> i32 { val tagged: Tag = Holder { value: 42 }; 42 }"),  Default::default()).unwrap();
    let mut program = artifact.records().program.clone();
    let table = program.modules[program.root.index()]
        .interface_tables
        .iter_mut()
        .find(|table| !table.arguments.is_empty())
        .unwrap();
    assert!(table.methods.is_empty());
    table.arguments[0] = Ty::Builtin(BuiltinType::F32);
    assert!(verify_program(&program, artifact.definitions(), &Default::default()).is_err());
    execution::assert_artifact_returns_i32(&engine, artifact, 42);
}

#[test]
fn malformed_generic_interface_instances_are_rejected_before_execution() {
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("generic-wire.kgr", "use std::hash::{Hash};\ntrait Reader { type Item; fn read(self) -> Self::Item; } struct Holder<T> { val value: T } impl<T: Eq + Hash> Reader for Holder<T> { type Item = T; fn read(self) -> T { self.value } } fn main() -> i32 { val a: Reader<Item = i32> = Holder { value: 42 }; val b: Reader<Item = String> = Holder { value: \"text\" }; a.read() }"),  Default::default()).unwrap();
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact.import_into(&context, &Default::default()).unwrap();
    let table = identity::collect(
        &artifact
            .records()
            .program
            .modules
            .iter()
            .collect::<Vec<_>>(),
        &context,
        &Default::default(),
    )
    .unwrap();
    verify_program(&artifact.records().program, &table, &Default::default()).unwrap();
    for mutation in 0..6 {
        let mut program = artifact.records().program.clone();
        let tables = &mut program.modules[program.root.index()].interface_tables;
        let index = tables
            .iter()
            .position(|table| !table.arguments.is_empty())
            .unwrap();
        match mutation {
            0 => tables[index].arguments.push(Ty::Builtin(BuiltinType::I32)),
            1 => tables[index].methods.clear(),
            2 => {
                let duplicate = tables[index].clone();
                tables.push(duplicate);
            }
            3 => tables[index].methods = tables[index + 1].methods.clone(),
            4 => tables[index].arguments[0] = Ty::Builtin(BuiltinType::F32),
            5 => {
                tables.remove(index);
            }
            _ => unreachable!(),
        }
        assert!(
            verify_program(&program, &context.snapshot(), &Default::default()).is_err(),
            "accepted mutation {mutation}"
        );
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err()
        );
    }
}
