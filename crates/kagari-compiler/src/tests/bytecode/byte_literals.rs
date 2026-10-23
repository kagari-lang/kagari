//! Byte spelling is gone before executable serialization; mutable arrays are not constants.

use std::collections::HashMap;

use crate::tests::common;
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, ConstantOperand},
};
use kagari_common::identity::map::DefinitionContext;
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn byte_literals_keep_fresh_make_array_sites_in_source_free_bytecode() {
    let bytes = {
        let context = DefinitionContext::new().unwrap();
        let program = common::adopt_bytecode(
            &context,
            common::bytecode_ok(
                r##"
                fn main() -> ([u8], [u8], [u8], [u8], [u8], [u8], [u8], u8) {
                    (b"\0\xFF", b"\0\xFF", b"", b"", br#"\xFF{name}"#, br"", br"", b'A')
                }
                "##,
            ),
        );
        KbcArtifact::from_program(program, Default::default(), &context, &Default::default())
            .unwrap()
            .to_bytes(&context.snapshot())
            .unwrap()
    };
    let fresh = DefinitionContext::new().unwrap();
    let artifact = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&fresh, &Default::default())
        .unwrap()
        .into_records();
    assert_eq!(artifact.to_bytes(&fresh.snapshot()).unwrap(), bytes);
    let verified = artifact
        .into_verified(&Default::default(), &fresh, &Default::default())
        .unwrap()
        .into_bytecode();
    let program = verified.program();
    let module = &program.modules[program.root.index()];
    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    let mut scalars = HashMap::new();
    let mut arrays = Vec::new();
    for instruction in &main.instructions {
        match instruction {
            BytecodeInstruction::LoadConst { dst, constant } => {
                if let ConstantOperand::I64(value) = &module.constants[constant.index()] {
                    scalars.insert(*dst, *value);
                } else {
                    scalars.remove(dst);
                }
            }
            BytecodeInstruction::Move { dst, src } => {
                if let Some(value) = scalars.get(src).copied() {
                    scalars.insert(*dst, value);
                } else {
                    scalars.remove(dst);
                }
            }
            BytecodeInstruction::MakeArray {
                element, elements, ..
            } => {
                assert_eq!(*element, Ty::Builtin(BuiltinType::U8));
                arrays.push(
                    elements
                        .iter()
                        .map(|slot| scalars[slot])
                        .collect::<Vec<_>>(),
                );
            }
            _ => {}
        }
    }
    assert_eq!(
        arrays,
        [
            vec![0, 255],
            vec![0, 255],
            vec![],
            vec![],
            b"\\xFF{name}"
                .iter()
                .map(|value| i64::from(*value))
                .collect(),
            vec![],
            vec![],
        ]
    );
    assert!(module.constants.contains(&ConstantOperand::I64(65)));
    assert!(
        !module
            .constants
            .iter()
            .any(|value| matches!(value, ConstantOperand::Str(_) | ConstantOperand::Char(_)))
    );
}
