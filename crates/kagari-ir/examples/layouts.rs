//! Inspect verified struct layouts without creating a runtime.
use kagari_common::SourceFile;
use kagari_hir::analyze_source;
use kagari_ir::{
    bytecode::{BytecodeInstruction, lower_to_bytecode},
    lower_to_ir,
};

fn main() {
    let source = SourceFile::new(
        "layouts.kgr",
        "struct Pair { var number: i32, val enabled: bool, val samples: [i32] } fn main() -> i32 { val p = Pair { enabled: true, number: 41, samples: [1, 2] }; if p.enabled { p.number += 1; }; p.number }",
    );
    let checked = analyze_source(&source, Default::default())
        .into_codegen()
        .unwrap();
    let ir = lower_to_ir(&checked, &Default::default()).unwrap();
    for layout in &ir.structures {
        println!("{}::{}", layout.declaration.module, layout.name());
        for (slot, field) in layout.fields.iter().enumerate() {
            println!(
                "  slot {slot}: {} {:?}, mutable={}",
                field.name, field.ty, field.mutable
            );
        }
    }
    let bytecode = lower_to_bytecode(&ir).unwrap();
    let (structure, fields) = bytecode
        .functions
        .iter()
        .flat_map(|f| &f.instructions)
        .find_map(|instruction| {
            if let BytecodeInstruction::MakeStruct {
                structure, fields, ..
            } = instruction
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
