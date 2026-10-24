use crate::{BytecodeArtifact, engine::KagariEngine};
use kagari_bytecode::program::{identity, verify_program};
use kagari_common::identity::map::DefinitionContext;
use kagari_source::source::SourceFile;
use kagari_types::{language::Protocol, scalar::BuiltinType, ty::Ty};

#[test]
fn executable_from_adapters_reject_forged_source_types_and_targets() {
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "memory://try-witness.kgr",
                "fn main()->Result<i32,i64> { val value: Result<i32,i32> = Err(42); Ok(value?) }",
            ),
            Default::default(),
        )
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let table = identity::collect(
        &artifact.program.modules.iter().collect::<Vec<_>>(),
        &context,
        &Default::default(),
    )
    .unwrap();
    verify_program(&artifact.program, &table, &Default::default()).unwrap();
    for mutation in 0..3 {
        let mut program = artifact.program.clone();
        let function = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .find(|function| {
                function
                    .metadata
                    .semantic
                    .protocol_adapter
                    .as_ref()
                    .is_some_and(|required| {
                        Protocol::from_definition(&required.interface.declaration, &table)
                            == Some(Protocol::From)
                    })
            })
            .expect("a checked numeric From adapter");
        let required = function
            .metadata
            .semantic
            .protocol_adapter
            .as_mut()
            .unwrap();
        match mutation {
            0 => required.interface.arguments[0] = Ty::Builtin(BuiltinType::String),
            1 => required.receiver = Ty::Builtin(BuiltinType::Bool),
            _ => {
                let parent = table.parent(required.member).unwrap().unwrap();
                let segment = table.resolve(required.member).unwrap().last().unwrap();
                required.member = context
                    .intern_child(parent, segment.kind, "branch", segment.occurrence)
                    .unwrap();
            }
        }
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err(),
            "forged adapter {mutation}"
        );
    }
}
