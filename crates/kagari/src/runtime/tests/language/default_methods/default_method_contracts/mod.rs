//! Structural operation contracts used by shared generic default methods.
use super::{editable_artifact, execute, execution};
use crate::BytecodeArtifact;
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
use kagari_contract::callable::witness::OperationWitness;
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn shared_tuple_comparison_composes_element_operations() {
    execute(SHARED_TUPLE_COMPARISON);
}

const SHARED_TUPLE_COMPARISON: &str = r#"
use std::hash::Hash;
use std::collections::list_content_equal;
fn same<T: PartialEq>(a: T, b: T) -> bool { a == b }
fn hashed<T: Hash>(value: T) -> i64 { value.hash() }
struct Item { val value: i32 }
impl PartialEq for Item { fn eq(self, other: Self) -> bool { true } }
impl Eq for Item {}
impl Hash for Item { fn hash(self) -> i64 { 7i64 } }
trait Compare {
    fn compare<T: PartialEq>(self, a: T, b: T) -> bool {
        (a, 7, a) == (b, 7, b) && !((a, 7) != (b, 7))
            && same((a, (7, a)), (b, (7, b)))
            && same(Vec::from([(a, 7)]), Vec::from([(b, 7)]))
            && list_content_equal(Vec::from([(a, 7)]), Vec::from([(b, 7)]))
    }
    fn hashes<T: Eq + Hash>(self, a: T, b: T) -> bool {
        (a, 7).hash() == (b, 7).hash() && hashed((a, (7, a))) == hashed((b, (7, b)))
            && hashed(Vec::from([(a, 7)])) == hashed(Vec::from([(b, 7)]))
    }
}
struct Source {}
impl Compare for Source {}
fn main() -> i32 {
    val source: Compare = Source {};
    val left = Item { value: 1 };
    val right = Item { value: 2 };
    if source.compare(left, right)
        && !source.compare(1, 2) && source.compare("same", "same")
        && source.hashes(left, right) && source.hashes(3, 3) && source.hashes("same", "same") { 42 } else { 0 }
}
"#;

#[test]
fn shared_tuple_protocol_metadata_is_verified_without_source() {
    let artifact = execution::engine(Default::default())
        .compile_to_artifact(
            SourceFile::new("shared-tuple.kgr", SHARED_TUPLE_COMPARISON),
            Default::default(),
        )
        .unwrap();
    let (context, artifact) = editable_artifact(artifact);
    for mutation in 0..4 {
        let mut program = artifact.program.clone();
        let semantic = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .map(|function| &mut function.metadata.semantic)
            .find(|semantic| semantic.protocol_adapter.is_some() && semantic.generic.is_some())
            .unwrap();
        match mutation {
            0 => semantic.generic.as_mut().unwrap().bounds.clear(),
            1 => semantic.generic.as_mut().unwrap().parameters[0].position += 1,
            2 => {
                semantic.protocol_adapter.as_mut().unwrap().receiver = Ty::Builtin(BuiltinType::I32)
            }
            3 => semantic
                .params
                .insert(0, Ty::Builtin(BuiltinType::I32))
                .map(|_| ())
                .unwrap(),
            _ => unreachable!(),
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

#[test]
fn composed_tuple_operation_witnesses_are_verified_without_source() {
    let artifact = execution::engine(Default::default())
        .compile_to_artifact(
            SourceFile::new("composed-tuple.kgr", SHARED_TUPLE_COMPARISON),
            Default::default(),
        )
        .unwrap();
    let (context, artifact) = editable_artifact(artifact);
    for mutation in 0..5 {
        let mut program = artifact.program.clone();
        let adapter = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| {
                let BytecodeInstruction::Call {
                    callee: CallTarget::Shared { contract, .. },
                    ..
                } = instruction
                else {
                    return None;
                };
                contract
                    .operations
                    .iter_mut()
                    .find_map(|operation| match operation {
                        OperationWitness::SharedAdapter(adapter) => Some(adapter),
                        _ => None,
                    })
            })
            .unwrap();
        match mutation {
            0 => adapter.call.operations.clear(),
            1 => adapter
                .call
                .operations
                .push(adapter.call.operations[0].clone()),
            2 => adapter.call.arguments[0] = Ty::Builtin(BuiltinType::Bool),
            3 => {
                let OperationWitness::Forward(required) = &mut adapter.call.operations[0] else {
                    panic!("forwarded component");
                };
                let Ty::Parameter { position, .. } = &mut required.receiver else {
                    panic!("component binder");
                };
                *position += 10;
            }
            4 => adapter.call.signature.result = Ty::Builtin(BuiltinType::I32),
            _ => unreachable!(),
        }
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err(),
            "composed witness mutation {mutation}"
        );
    }
    for mutation in 0..3 {
        let mut program = artifact.program.clone();
        let selected = program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.functions)
            .flat_map(|function| &mut function.instructions)
            .find_map(|instruction| {
                let BytecodeInstruction::Call {
                    callee: CallTarget::Shared { contract, .. },
                    ..
                } = instruction
                else {
                    return None;
                };
                contract
                    .operations
                    .iter_mut()
                    .find_map(|operation| match operation {
                        OperationWitness::SharedMethod(selected)
                            if !selected.operations.is_empty() =>
                        {
                            Some(selected)
                        }
                        _ => None,
                    })
            })
            .expect("conditional receiver operation fixture");
        match mutation {
            0 => selected.operations.clear(),
            1 => selected.operations.push(selected.operations[0].clone()),
            2 => {
                let OperationWitness::SharedAdapter(adapter) = &mut selected.operations[0] else {
                    panic!("tuple element operation");
                };
                let OperationWitness::Forward(required) = &mut adapter.call.operations[0] else {
                    panic!("forwarded element");
                };
                let Ty::Parameter { position, .. } = &mut required.receiver else {
                    panic!("element binder");
                };
                *position += 10;
            }
            _ => unreachable!(),
        }
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &context,
                &Default::default()
            )
            .is_err(),
            "conditional receiver dependency mutation {mutation}"
        );
    }
}
