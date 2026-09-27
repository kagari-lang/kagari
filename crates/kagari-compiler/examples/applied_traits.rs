//! Inspect a checked generic trait implementation without starting a runtime.
use kagari_abi::scalar::BuiltinType;
use kagari_abi::types::AbiType;
use kagari_abi::types::PublicAbiItem;
use kagari_bytecode::ArtifactBuildOptions;
use kagari_bytecode::ArtifactCompatibility;
use kagari_bytecode::BytecodeProgram;
use kagari_bytecode::KbcArtifact;
use kagari_bytecode::ModuleRef;
use kagari_common::SourceFile;
use kagari_compiler::bytecode::lower_to_bytecode;
use kagari_compiler::lower_to_mir;
use kagari_hir::analyze_source;

fn main() {
    let source = SourceFile::new(
        "applied_traits.kgr",
        "pub trait Echo<T> { fn get(self) -> T; } pub struct Pair { val number: i32 } impl Echo<i32> for Pair { fn get(self) -> i32 { self.number } } fn read<U: Echo<i32>>(value: U) -> i32 { value.get() } fn main() -> i32 { read(Pair { number: 42 }) }",
    );
    let checked = analyze_source(&source, Default::default())
        .into_codegen()
        .expect("checked applied trait implementation");
    let ir = lower_to_mir(&checked, &Default::default()).expect("verified IR");
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
        if ty.arguments == [AbiType::Builtin(BuiltinType::I32)]));
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
