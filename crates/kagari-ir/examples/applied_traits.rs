//! Inspect a checked generic trait implementation without starting a runtime.
use kagari_common::SourceFile;
use kagari_hir::analyze_source;
use kagari_ir::{
    bytecode::{
        ArtifactBuildOptions, ArtifactCompatibility, BytecodeProgram, KbcArtifact, ModuleRef,
        lower_to_bytecode,
    },
    lower_to_ir,
    module::{PublicAbiItem, abi::AbiType},
};

fn main() {
    let source = SourceFile::new(
        "applied_traits.kgr",
        "pub trait Echo<T> { fn get(self) -> T; } pub struct Pair { val number: i32 } impl Echo<i32> for Pair { fn get(self) -> i32 { self.number } } fn read<U: Echo<i32>>(value: U) -> i32 { value.get() } fn main() -> i32 { read(Pair { number: 42 }) }",
    );
    let checked = analyze_source(&source, Default::default())
        .into_codegen()
        .expect("checked applied trait implementation");
    let ir = lower_to_ir(&checked, &Default::default()).expect("verified IR");
    let bytecode = lower_to_bytecode(&ir).expect("verified bytecode");
    let table = bytecode
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicAbiItem::InterfaceTable(table) => Some(table),
            _ => None,
        })
        .expect("implementation ABI");
    assert!(matches!(&table.trait_type, AbiType::Trait(ty)
        if ty.arguments == [AbiType::Builtin(kagari_hir::types::BuiltinType::I32)]));
    let executable = &bytecode.interface_tables[0];
    assert_eq!(executable.declaration, table.declaration);
    assert_eq!(executable.methods.len(), 1);
    assert_eq!(
        bytecode.functions[executable.methods[0].function.index()]
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
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![bytecode],
        },
        ArtifactBuildOptions::default(),
    )
    .expect("artifact");
    KbcArtifact::from_bytes(&artifact.to_bytes().expect("encoded artifact"))
        .expect("decoded artifact")
        .validate_for_loader(&ArtifactCompatibility::default())
        .expect("loadable artifact");
}
