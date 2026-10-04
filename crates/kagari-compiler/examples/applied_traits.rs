//! Inspect a checked generic trait implementation without starting a runtime.
use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, ArtifactCompatibility, KbcArtifact},
    module::CallableTarget,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::types::PublicItem;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn main() {
    let source = SourceFile::new(
        "applied_traits.kgr",
        "pub trait Echo<T> { fn get(self) -> T; } pub struct Pair { val number: i32 } impl Echo<i32> for Pair { fn get(self) -> i32 { self.number } } fn read<U: Echo<i32>>(value: U) -> i32 { value.get() } fn main() -> i32 { read(Pair { number: 42 }) }",
    );
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(source.name(), source.text().into(), SourceLayer::Base)
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), &Default::default())
        .expect("analysis snapshot");
    let checked = snapshot
        .check_program(root, &Default::default())
        .expect("checked applied trait implementation");
    let ir = lower_program_to_mir(&checked, &Default::default()).expect("verified IR");
    let program = lower_program_to_bytecode(&ir).expect("verified bytecode");
    let bytecode = &program.modules[program.root.index()];
    let table = bytecode
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::InterfaceTable(table) => Some(table),
            _ => None,
        })
        .expect("implementation ABI");
    assert!(matches!(&table.trait_type, Ty::Trait(ty)
        if ty.arguments == [Ty::Builtin(BuiltinType::I32)]));
    let executable = bytecode
        .interface_tables
        .iter()
        .find(|executable| executable.declaration == table.declaration)
        .expect("selected implementation table");
    assert_eq!(executable.declaration, table.declaration);
    assert_eq!(executable.methods.len(), 1);
    let CallableTarget::Script(function) = executable.methods[0].target else {
        panic!("script implementation")
    };
    assert_eq!(
        bytecode.functions[function.index()]
            .identity
            .as_ref()
            .unwrap()
            .declaration
            .path
            .last()
            .unwrap()
            .name,
        "get"
    );
    println!(
        "{}: {} verified method slot",
        table.name,
        executable.methods.len()
    );
    let artifact =
        KbcArtifact::from_program(program, ArtifactBuildOptions::default()).expect("artifact");
    KbcArtifact::from_bytes(&artifact.to_bytes().expect("encoded artifact"))
        .expect("decoded artifact")
        .validate_for_loader(&ArtifactCompatibility::default())
        .expect("loadable artifact");
}
