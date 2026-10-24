use crate::{BytecodeArtifact, engine::KagariEngine};
use kagari_bytecode::instruction::BytecodeInstruction;
use kagari_common::identity::map::DefinitionContext;
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn malformed_nominal_enum_operations_are_rejected_before_execution() {
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new("verified.kgr", "fn main()->Option<i32> { Some(42) }"),
            Default::default(),
        )
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    for case in 0..5 {
        let mut program = artifact.program.clone();
        let instruction = program
            .modules
            .iter_mut()
            .flat_map(|m| &mut m.functions)
            .flat_map(|f| &mut f.instructions)
            .find(|i| matches!(i, BytecodeInstruction::MakeEnum { .. }))
            .unwrap();
        let BytecodeInstruction::MakeEnum {
            variant,
            arguments,
            fields,
            ..
        } = instruction
        else {
            unreachable!()
        };
        match case {
            0 => *variant = 9,
            1 => arguments.clear(),
            2 => fields.clear(),
            3 => arguments[0] = Ty::Builtin(BuiltinType::Bool),
            _ => *variant = 1,
        }
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err(),
            "case {case}"
        );
    }
}
