//! Structural operation contracts used by shared generic default methods.
use super::{editable_artifact, execution};
use crate::BytecodeArtifact;
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
use kagari_contract::{
    callable::{construction::evidence::ConstructionSelection, witness::OperationWitness},
    types::PublicItem,
};
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};

const SHARED_TUPLE_COMPARISON: &str = include_str!(
    "../../../../../../tests/fixtures/language/default_methods/tuple_operations/shared_tuple_comparison_composes_element_operations.kgr"
);

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

#[test]
fn shared_tuple_construction_capabilities_are_verified_without_source() {
    let artifact = execution::engine(Default::default())
        .compile_to_artifact(
            SourceFile::new("tuple-construction.kgr", SHARED_TUPLE_COMPARISON),
            Default::default(),
        )
        .unwrap();
    let (context, artifact) = editable_artifact(artifact);
    for mutation in 0..4 {
        let mut program = artifact.program.clone();
        if mutation < 2 {
            let table = program
                .modules
                .iter_mut()
                .flat_map(|module| &mut module.public_items)
                .find_map(|item| match item {
                    PublicItem::InterfaceTable(table)
                        if table.bridge.is_some() && !table.generic_params.is_empty() =>
                    {
                        Some(table)
                    }
                    _ => None,
                })
                .expect("generic intrinsic tuple table");
            match mutation {
                0 => table.bounds.clear(),
                1 => {
                    let Ty::Tuple(items) = &mut table.for_type else {
                        panic!("tuple receiver");
                    };
                    items.swap(0, 1);
                }
                _ => unreachable!(),
            }
        } else {
            let witness = program
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
                    contract.constructions.iter_mut().find(|witness| {
                        matches!(&witness.requirement.receiver, Ty::Tuple(_))
                            && !witness.requirement.receiver.is_concrete()
                    })
                })
                .expect("composed tuple construction");
            if mutation == 2 {
                let ConstructionSelection::Declared { constructions, .. } = &mut witness.selection
                else {
                    panic!("selected tuple table");
                };
                assert!(!constructions.is_empty());
                constructions.clear();
            } else {
                witness.selection = ConstructionSelection::Forward;
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
            "tuple construction mutation {mutation}"
        );
    }
}
