use crate::{BytecodeArtifact, engine::KagariEngine};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    program::{identity, verify_program},
};
use kagari_common::identity::map::DefinitionContext;
use kagari_source::source::SourceFile;
use kagari_types::{
    language::binding::option_declaration,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};

#[test]
fn malformed_native_iter_operations_are_rejected_before_execution() {
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "iter-wire.kgr",
                "fn main()->i32 {var total=0;for x in [20,22]{total+=x;}total}",
            ),
            Default::default(),
        )
        .unwrap();
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
    let root = artifact.records().program.root.index();
    let import = artifact.records().program.modules[root]
        .native_imports
        .iter()
        .position(|import| {
            artifact
                .definitions()
                .resolve(import.binding)
                .unwrap()
                .last()
                .is_some_and(|part| part.name == "iter")
        })
        .unwrap();
    for corrupt in 0..3 {
        let mut program = artifact.records().program.clone();
        match corrupt {
            0 => {
                program.modules[root].native_imports[import]
                    .signature
                    .params[0] = Ty::Builtin(BuiltinType::I32)
            }
            1 => {
                let instruction = program.modules[root]
                    .functions
                    .iter_mut()
                    .flat_map(|function| &mut function.instructions)
                    .find(|instruction| {
                        matches!(instruction, BytecodeInstruction::Call {
                        callee: CallTarget::Native(id), ..
                    } if id.index() == import)
                    })
                    .unwrap();
                let BytecodeInstruction::Call { args, .. } = instruction else {
                    unreachable!()
                };
                args.clear();
            }
            _ => {
                program.modules[root].native_imports[import]
                    .signature
                    .params[0] = Ty::Array(Box::new(Ty::Enum(NominalTy {
                    declaration: artifact
                        .definitions()
                        .lookup(&option_declaration())
                        .unwrap(),
                    arguments: vec![],
                    associated_types: Default::default(),
                })))
            }
        }
        assert!(verify_program(&program, &context.snapshot(), &Default::default()).is_err());
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
