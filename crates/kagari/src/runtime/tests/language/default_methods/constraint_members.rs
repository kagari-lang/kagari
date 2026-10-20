//! Generic constraint member dispatch and source-free witness validation.
use super::*;

#[test]
fn shared_constraint_calls_a_generic_member_and_preserves_its_override() {
    execute(
        r#"
trait Identity { fn identity<T>(self, value: T) -> T { value } }
struct Source { var calls: i32 }
impl Identity for Source {
    fn identity<T>(self, value: T) -> T { self.calls += 1; value }
}
fn forward<S: Identity, T>(source: S, value: T) -> T { source.identity(value) }
trait Relay {
    fn relay<S: Identity, T>(self, source: S, value: T) -> T {
        forward(source, value)
    }
}
impl Relay for i32 {}
struct Item { val value: i32 }
fn main() -> i32 {
    val relay: Relay = 0;
    val source = Source { calls: 0 };
    val result = relay.relay(source, Item { value: 42 });
    val text: String = relay.relay(source, "checked");
    if source.calls == 2 && text == "checked" { result.value } else { 0 }
}
"#,
    );
}

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

#[test]
fn generic_constraint_members_receive_their_local_bound_operations() {
    execute(
        r#"use std::cmp::{Ordering};

trait Compare<R> { fn choose<K: Ord>(self, left: K, right: K) -> K; }
struct Source<R> { val value: R }
impl<R> Compare<R> for Source<R> {
    fn choose<K: Ord>(self, left: K, right: K) -> K {
        if left.cmp(right) == Ordering::Greater { left } else { right }
    }
}
trait Relay {
    fn relay<S: Compare<i32>, K: Ord>(self, source: S, left: K, right: K) -> K {
        source.choose(left, right)
    }
}
impl Relay for i32 {}
fn main() -> i32 {
    val relay: Relay = 0;
    val source = Source { value: 1 };
    val text: String = relay.relay(source, "a", "b");
    if text == "b" { relay.relay(source, 21, 42) } else { 0 }
}
"#,
    );
}
