use super::{Node, OperationWitnessRecord};
use crate::{
    callable::{
        shared::SharedCall,
        wire::{SharedCallEntryRecord, SharedMethodWitnessRecord},
        witness::SharedMethodWitness,
    },
    types::ConcreteFunctionIdentity,
};
use bincode::{DefaultOptions, Options};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, mapping::DefinitionMappingError,
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{
        requirement::NativeCallableRequirement, wire::native::NativeCallableRequirementRecord,
    },
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};

#[test]
fn flat_operation_records_reject_invalid_tree_shapes_and_depth() {
    let context = DefinitionContext::new().unwrap();
    let root = context
        .intern_root(&ModuleIdentity::single_file("operation-wire.kgr"))
        .unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Trait, "Protocol", 0)
        .unwrap();
    let member = context
        .intern_child(declaration, DefinitionKind::Method, "eq", 0)
        .unwrap();
    let function = context
        .intern_child(root, DefinitionKind::Function, "adapter", 0)
        .unwrap();
    let encoding = context
        .snapshot()
        .encode([declaration, member, function])
        .unwrap();
    let decoding = encoding.table.decode().unwrap();
    // Codec shape is independent of the later semantic adapter proof.
    let requirement = NativeCallableRequirement {
        receiver: Ty::Builtin(BuiltinType::I32),
        interface: NominalTy {
            declaration,
            arguments: vec![],
            associated_types: Default::default(),
        },
        member,
        arguments: vec![],
    };
    let call = SharedCall {
        instance: ConcreteFunctionIdentity {
            declaration: function,
            arguments: vec![],
        },
        implementation: CallableImplementation::Script,
        arguments: vec![requirement.receiver.clone()],
        signature: Signature {
            params: vec![requirement.receiver.clone(); 2],
            result: Ty::Builtin(BuiltinType::Bool),
        },
        operations: vec![],
    };
    let leaf = || {
        Node::Forward(Box::new(
            NativeCallableRequirementRecord::encode(&requirement, &encoding, &Default::default())
                .unwrap(),
        ))
    };
    let parent = |children| Node::SharedAdapter {
        requirement: NativeCallableRequirementRecord::encode(
            &requirement,
            &encoding,
            &Default::default(),
        )
        .unwrap(),
        entry: Box::new(
            SharedCallEntryRecord::encode(&call, &encoding, &Default::default()).unwrap(),
        ),
        children,
    };
    let method = |children| Node::SharedMethod {
        entry: Box::new(
            SharedMethodWitnessRecord::encode(
                &SharedMethodWitness {
                    requirement: requirement.clone(),
                    implementation: call.instance.clone(),
                    operations: vec![],
                },
                &encoding,
                &Default::default(),
            )
            .unwrap(),
        ),
        children,
    };
    for nodes in [
        vec![],
        vec![leaf(), leaf()],
        vec![parent(2), leaf()],
        vec![method(2), leaf()],
    ] {
        assert!(matches!(
            OperationWitnessRecord { nodes }.decode(&decoding, &Default::default()),
            Err(DefinitionMappingError::InvalidContract)
        ));
    }
    for depth in [64, 65] {
        let nodes = (0..depth)
            .map(|index| if index % 2 == 0 { parent(1) } else { method(1) })
            .chain([leaf()])
            .collect();
        let bytes = DefaultOptions::new()
            .with_fixint_encoding()
            .serialize(&OperationWitnessRecord { nodes })
            .unwrap();
        let record: OperationWitnessRecord = DefaultOptions::new()
            .with_fixint_encoding()
            .deserialize(&bytes)
            .unwrap();
        let decoded = record.decode(&decoding, &Default::default());
        if depth == 64 {
            assert_eq!(decoded.unwrap().walk().count(), 65);
        } else {
            assert!(matches!(
                decoded,
                Err(DefinitionMappingError::LimitExceeded)
            ));
        }
    }
    assert!(
        DefaultOptions::new()
            .with_fixint_encoding()
            .deserialize::<OperationWitnessRecord>(&4097u64.to_le_bytes())
            .is_err()
    );
}
