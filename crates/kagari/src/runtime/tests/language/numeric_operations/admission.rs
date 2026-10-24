use crate::engine::KagariEngine;
use kagari_bytecode::instruction::BytecodeInstruction;
use kagari_common::identity::map::DefinitionContext;
use kagari_source::source::SourceFile;
use kagari_types::scalar::BuiltinType;

#[test]
fn invalid_numeric_artifact_contracts_are_rejected_before_execution() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("conversion.kgr", "fn main() -> u8 { 256u16 as u8 }"),
            Default::default(),
        )
        .unwrap();
    let definitions = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&definitions, &Default::default())
        .unwrap()
        .into_records();
    artifact
        .clone()
        .validate_for_loader(&Default::default(), &definitions, &Default::default())
        .unwrap();
    for target in [BuiltinType::Bool, BuiltinType::String] {
        let mut forged = artifact.clone();
        let conversion = forged
            .program
            .modules
            .iter_mut()
            .flat_map(|m| &mut m.functions)
            .flat_map(|f| &mut f.instructions)
            .find_map(|instruction| match instruction {
                BytecodeInstruction::Convert { conversion, .. } => Some(conversion),
                _ => None,
            })
            .unwrap();
        conversion.target = target;
        assert!(
            forged
                .validate_for_loader(&Default::default(), &definitions, &Default::default())
                .is_err()
        );
    }
}
