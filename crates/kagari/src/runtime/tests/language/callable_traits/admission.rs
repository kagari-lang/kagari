use crate::{BytecodeArtifact, engine::KagariEngine};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    program::{identity, verify_program},
};
use kagari_common::identity::map::DefinitionContext;
use kagari_contract::callable::witness::OperationWitness;
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn forged_constraint_operations_are_rejected_before_execution() {
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "witnesses.kgr",
                r#"
trait Invoke { fn invoke<F: Fn(i32) -> i32>(self, f: F) -> i32 { f(21) } }
struct Runner {}
impl Invoke for Runner {}
fn main() -> i32 { val runner: Invoke = Runner {}; runner.invoke(|x| x * 2) }
"#,
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
    for mutation in 0..5 {
        let mut program = artifact.program.clone();
        let contract = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| match instruction {
                BytecodeInstruction::Call {
                    callee: CallTarget::InterfaceMethod { contract, .. },
                    ..
                } if !contract.operations.is_empty() => Some(contract),
                _ => None,
            })
            .unwrap();
        match mutation {
            0 => contract.operations.clear(),
            1 => contract.operations.push(contract.operations[0].clone()),
            2 => {
                let OperationWitness::Selected(selected) = &mut contract.operations[0] else {
                    panic!("selected operation");
                };
                selected.signature.result = Ty::Builtin(BuiltinType::Bool);
            }
            3 => {
                let OperationWitness::Selected(selected) = &mut contract.operations[0] else {
                    panic!("selected operation");
                };
                let view = table.resolve(selected.instance.declaration).unwrap();
                let mut parent = context.intern_root(view.module()).unwrap();
                for (index, segment) in view.segments().enumerate() {
                    let name = if index == 0 { "main" } else { segment.name };
                    parent = context
                        .intern_child(parent, segment.kind, name, segment.occurrence)
                        .unwrap();
                }
                selected.instance.declaration = parent;
            }
            _ => {
                contract.operations[0] = OperationWitness::Forward(Box::new(
                    contract.operations[0].requirement().clone(),
                ))
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
            "forged witness {mutation}"
        );
    }
}
