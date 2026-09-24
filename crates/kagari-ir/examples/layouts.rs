//! Inspect verified struct layouts and interface identities without a runtime.
use kagari_common::SourceFile;
use kagari_hir::analyze_source;
use kagari_ir::{
    bytecode::{BytecodeInstruction, lower_to_bytecode},
    lower_to_ir,
};

fn main() {
    let source = SourceFile::new(
        "layouts.kgr",
        "pub struct Deferred<T> { val payload: T } pub struct Pair { var number: i32, val enabled: bool, val samples: [i32] } pub trait Number { fn get(self) -> i32; } impl Number for Pair { fn get(self) -> i32 { self.number } } impl<T> Number for Deferred<T> { fn get(self) -> i32 { 7 } } fn read<T: Number>(value: T) -> i32 { value.get() } fn main() -> i32 { read(Deferred { payload: 1 }); val p = Pair { enabled: true, number: 41, samples: [1, 2] }; if p.enabled { p.number += 1; }; p.number }",
    );
    let checked = analyze_source(&source, Default::default())
        .into_codegen()
        .unwrap();
    let ir = lower_to_ir(&checked, &Default::default()).unwrap();
    for layout in &ir.structures {
        assert_eq!(layout.declaration.path.len(), 1);
        assert_eq!(layout.declaration.path[0].occurrence, 0);
        println!("{}::{}", layout.declaration.module, layout.name());
        for (slot, field) in layout.fields.iter().enumerate() {
            assert_eq!(field.declaration.path.len(), 2);
            assert_eq!(field.declaration.path[1].occurrence, 0);
            println!(
                "  slot {slot}: {} {:?}, mutable={}",
                field.name, field.ty, field.mutable
            );
        }
    }
    let cancelled = kagari_common::cancellation::CancellationToken::default();
    cancelled.cancel();
    assert_eq!(
        kagari_ir::module::verify_ir(ir.clone().into_unverified(), &cancelled)
            .unwrap_err()
            .kind,
        kagari_ir::module::IrVerificationErrorKind::Cancelled,
    );
    let bytecode = lower_to_bytecode(&ir).unwrap();
    assert!(bytecode.functions.iter().all(|function| {
        function.identity.as_ref().is_some_and(|identity| {
            identity.declaration.module == bytecode.identity
                && bytecode.function_table[function.id.index()].identity == function.identity
        })
    }));
    let interface = bytecode
        .public_items
        .iter()
        .find_map(|item| match item {
            kagari_ir::module::PublicAbiItem::InterfaceTable(table) => Some(table),
            _ => None,
        })
        .expect("checked interface declaration");
    assert_eq!(interface.declaration.module, bytecode.identity);
    assert_eq!(
        interface.declaration.path[0].kind,
        kagari_common::identity::DefinitionKind::Impl
    );
    let executable_table = bytecode
        .interface_tables
        .iter()
        .find(|table| table.declaration == interface.declaration)
        .expect("executable interface table");
    assert_eq!(executable_table.methods.len(), 1);
    let method = &executable_table.methods[0];
    assert_eq!(method.method.path.last().unwrap().name, "get");
    assert_eq!(
        bytecode.functions[method.function.index()]
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
    let generic_table = bytecode
        .interface_tables
        .iter()
        .find(|table| table.declaration != interface.declaration)
        .expect("specialized generic interface table");
    assert_eq!(generic_table.methods.len(), 1);
    assert_eq!(
        bytecode.functions[generic_table.methods[0].function.index()]
            .identity
            .as_ref()
            .unwrap()
            .arguments,
        [kagari_ir::module::abi::AbiType::Builtin(
            kagari_hir::types::BuiltinType::I32
        )]
    );
    println!(
        "interface {} has a stable declaration identity and executable method slot",
        interface.name
    );
    println!(
        "{} executable functions retain their declaration identities",
        bytecode.functions.len()
    );
    let (structure, fields) = bytecode
        .functions
        .iter()
        .flat_map(|f| &f.instructions)
        .find_map(|instruction| {
            if let BytecodeInstruction::MakeStruct {
                structure, fields, ..
            } = instruction
                && bytecode.structures[structure.index()].name() == "Pair"
            {
                Some((structure, fields))
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        bytecode.structures[structure.index()]
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["number", "enabled", "samples"]
    );
    assert_eq!(
        fields.len(),
        bytecode.structures[structure.index()].fields.len()
    );
}
