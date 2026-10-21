use super::{EvidenceRecord, Node, Selection};
use crate::callable::construction::{
    evidence::ConstructionRequirement, wire::ConstructionRequirementRecord,
};
use crate::types::wire::module::ConcreteFunctionIdentityRecord;
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
        constructions: vec![],
        receiver_access: None,
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
        operations: children,
        constructions: 0,
    };
    let method = |children| Node::SharedMethod {
        entry: Box::new(
            SharedMethodWitnessRecord::encode(
                &SharedMethodWitness {
                    constructions: vec![],
                    requirement: requirement.clone(),
                    implementation: call.instance.clone(),
                    operations: vec![],
                },
                &encoding,
                &Default::default(),
            )
            .unwrap(),
        ),
        operations: children,
        constructions: 0,
    };
    for nodes in [
        vec![],
        vec![leaf(), leaf()],
        vec![parent(2), leaf()],
        vec![method(2), leaf()],
    ] {
        assert!(matches!(
            EvidenceRecord { nodes }.decode_operation(&decoding, &Default::default()),
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
            .serialize(&EvidenceRecord { nodes })
            .unwrap();
        let record: EvidenceRecord = DefaultOptions::new()
            .with_fixint_encoding()
            .deserialize(&bytes)
            .unwrap();
        let decoded = record.decode_operation(&decoding, &Default::default());
        if depth == 64 {
            assert_eq!(decoded.unwrap().walk().count(), 65);
        } else {
            assert!(matches!(
                decoded,
                Err(DefinitionMappingError::LimitExceeded)
            ));
        }
    }
    let construction = |operations| Node::Construction {
        requirement: ConstructionRequirementRecord::encode(
            &ConstructionRequirement {
                receiver: requirement.receiver.clone(),
                interface: requirement.interface.clone(),
            },
            &encoding,
            &Default::default(),
        )
        .unwrap(),
        selection: Selection::Declared {
            implementation: ConcreteFunctionIdentityRecord::encode(
                &call.instance,
                &encoding,
                &Default::default(),
            )
            .unwrap(),
            operations,
            constructions: 0,
        },
    };
    let constructor_method = || {
        let mut node = method(0);
        if let Node::SharedMethod { constructions, .. } = &mut node {
            *constructions = 1;
        }
        node
    };
    for nodes in [
        vec![method(1), construction(0)],
        vec![constructor_method(), leaf()],
        vec![construction(0)],
    ] {
        assert!(matches!(
            EvidenceRecord { nodes }.decode_operation(&decoding, &Default::default()),
            Err(DefinitionMappingError::InvalidContract)
        ));
    }
    for depth in [64, 65] {
        let nodes = (0..depth)
            .map(|index| {
                if index % 2 == 0 {
                    constructor_method()
                } else {
                    construction(1)
                }
            })
            .chain([if depth % 2 == 0 {
                leaf()
            } else {
                construction(0)
            }])
            .collect();
        let decoded = EvidenceRecord { nodes }.decode_operation(&decoding, &Default::default());
        if depth == 64 {
            assert_eq!(decoded.unwrap().walk().count(), 33);
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
            .deserialize::<EvidenceRecord>(&4097u64.to_le_bytes())
            .is_err()
    );
}
