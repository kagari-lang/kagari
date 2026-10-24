//! Shared generic construction has its own capability, including methodless traits.
use super::editable_artifact;
use crate::{BytecodeArtifact, test_support::execution};
use kagari_bytecode::instruction::{BytecodeInstruction, ConstructionTarget};
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn bound_construction_requires_the_declared_capability_and_actual_input_type() {
    let artifact = execution::engine(Default::default())
        .compile_to_artifact(
            SourceFile::new(
                "bound-construction.kgr",
                r#"
trait Read { fn read(self) -> i32; }
impl Read for i32 { fn read(self) -> i32 { self } }
trait Erase {
    fn erase<T: Read, U>(self, value: T, other: U) -> Read { value }
}
impl Erase for i32 {}
fn main() -> i32 { val eraser: Erase = 0; eraser.erase(42, true).read() }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let (context, artifact) = editable_artifact(artifact);
    for mutation in 0..5 {
        let mut program = artifact.program.clone();
        let function = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .find(|function| {
                function.instructions.iter().any(|instruction| {
                    matches!(
                        instruction,
                        BytecodeInstruction::MakeInterface {
                            target: ConstructionTarget::Bound(_),
                            ..
                        }
                    )
                })
            })
            .unwrap();
        if mutation == 0 {
            function
                .metadata
                .semantic
                .generic
                .as_mut()
                .unwrap()
                .bounds
                .clear();
        } else {
            let other = function.metadata.parameter_slots[2];
            let (input, required) = function
                .instructions
                .iter_mut()
                .find_map(|instruction| match instruction {
                    BytecodeInstruction::MakeInterface {
                        value,
                        target: ConstructionTarget::Bound(required),
                        ..
                    } => Some((value, required)),
                    _ => None,
                })
                .unwrap();
            match mutation {
                1 => *input = other,
                2 => required.receiver = Ty::Builtin(BuiltinType::Bool),
                3 => {
                    let Ty::Parameter { position, .. } = &mut required.receiver else {
                        panic!("generic receiver")
                    };
                    *position += 20;
                }
                4 => required
                    .interface
                    .arguments
                    .push(Ty::Builtin(BuiltinType::Bool)),
                _ => unreachable!(),
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
            "mutation {mutation}"
        );
    }
}
