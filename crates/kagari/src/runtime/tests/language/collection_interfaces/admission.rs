use crate::engine::KagariEngine;
use kagari_bytecode::{artifact::KbcArtifact, instruction::BytecodeInstruction};
use kagari_common::identity::map::DefinitionContext;
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn iterator_view_adaptation_rejects_forged_output_types() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "iterator-view-proof.kgr",
                "fn main() -> Iterable<Item = i32> { Vec::from([42]) }",
            ),
            Default::default(),
        )
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    for wrong_result in [false, true] {
        let mut program = artifact.program.clone();
        let target = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| match instruction {
                BytecodeInstruction::UpcastInterface { source, target, .. }
                    if source.declaration == target.declaration && source != target =>
                {
                    Some(target)
                }
                _ => None,
            })
            .expect("checked iterator-result adaptation");
        let result = target
            .associated_types
            .values_mut()
            .find(|value| matches!(value, Ty::Trait(_)))
            .expect("erased iterator result");
        if wrong_result {
            *result = Ty::Builtin(BuiltinType::I32);
        } else {
            let Ty::Trait(iterator) = result else {
                unreachable!()
            };
            *iterator.associated_types.values_mut().next().unwrap() = Ty::Builtin(BuiltinType::I64);
        }
        assert!(
            KbcArtifact::from_program(program, Default::default(), &context, &Default::default())
                .is_err(),
            "forged iterator output (wrong result category: {wrong_result})"
        );
    }
}
