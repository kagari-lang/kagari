//! Generic constraint member dispatch and source-free witness validation.
use super::*;

#[test]
fn forged_shared_constraint_method_selections_are_rejected_without_source() {
    for source in [
        r#"
trait Identity { fn identity<T>(self, value: T) -> T { value } }
impl Identity for i32 {}
trait Relay { fn relay<S: Identity>(self, source: S) -> i32 { source.identity(42) } }
impl Relay for i32 {}
fn main() -> i32 { val relay: Relay = 0; relay.relay(0) }
"#,
        SHARED_RECEIVER_COMPARISON,
    ] {
        let artifact = execution::engine(Default::default())
            .compile_to_artifact(
                SourceFile::new("generic-witness.kgr", source),
                Default::default(),
            )
            .unwrap();
        let (context, artifact) = editable_artifact(artifact);
        for mutation in 0..4 {
            let mut program = artifact.program.clone();
            let selected = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.functions)
                .flat_map(|function| &mut function.instructions)
                .find_map(|instruction| {
                    let operations = match instruction {
                        BytecodeInstruction::Call {
                            callee: CallTarget::InterfaceMethod { contract, .. },
                            ..
                        } => &mut contract.operations,
                        BytecodeInstruction::Call {
                            callee: CallTarget::Shared { contract, .. },
                            ..
                        } => &mut contract.operations,
                        _ => return None,
                    };
                    operations.iter_mut().find_map(|operation| match operation {
                        OperationWitness::SharedMethod(selected) => Some(selected),
                        _ => None,
                    })
                })
                .unwrap();
            let identity = selected.implementation.clone();
            match mutation {
                0 => {
                    selected.requirement.member =
                        renamed(&context, selected.requirement.member, "missing")
                }
                1 => {
                    let id = selected.implementation.declaration;
                    let table = context.snapshot();
                    let parent = table.parent(id).unwrap().unwrap();
                    let segment = table.resolve(id).unwrap().last().unwrap();
                    selected.implementation.declaration = context
                        .intern_child(parent, segment.kind, segment.name, segment.occurrence + 1)
                        .unwrap();
                }
                2 => selected
                    .implementation
                    .arguments
                    .push(selected.requirement.receiver.clone()),
                _ => {
                    assert!(program.modules.iter().any(|module| {
                        module
                            .interface_tables
                            .iter()
                            .any(|table| table.declaration == identity.declaration)
                    }));
                    for module in &mut program.modules {
                        module
                            .interface_tables
                            .retain(|table| table.declaration != identity.declaration);
                    }
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
                "forged shared method {mutation}"
            );
        }
    }
}
