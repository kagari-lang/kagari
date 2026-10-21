use super::*;
use crate::callable::construction::evidence::ConstructionRequirement;
use crate::types::ConcreteFunctionIdentity;
use bincode::{DefaultOptions, Options};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, scoped::DefinitionReferences,
};
use kagari_types::{
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};

#[test]
fn construction_records_reject_forged_shapes_and_bound_depth() {
    let context = DefinitionContext::new().unwrap();
    let root = context
        .intern_root(&ModuleIdentity::single_file("construction-wire.kgr"))
        .unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Trait, "Marker", 0)
        .unwrap();
    let implementation = context
        .intern_child(root, DefinitionKind::Impl, "MarkerForI32", 0)
        .unwrap();
    let encoding = context
        .snapshot()
        .encode([declaration, implementation])
        .unwrap();
    let decoding = encoding.table.decode().unwrap();
    let requirement = ConstructionRequirement {
        receiver: Ty::Builtin(BuiltinType::I32),
        interface: NominalTy {
            declaration,
            arguments: vec![],
            associated_types: Default::default(),
        },
    };
    let node = |children| Node::Construction {
        requirement: ConstructionRequirementRecord::encode(
            &requirement,
            &encoding,
            &Default::default(),
        )
        .unwrap(),
        selection: Selection::Declared {
            implementation: ConcreteFunctionIdentityRecord::encode(
                &ConcreteFunctionIdentity {
                    declaration: implementation,
                    arguments: vec![],
                },
                &encoding,
                &Default::default(),
            )
            .unwrap(),
            operations: 0,
            constructions: children,
        },
    };
    for nodes in [vec![], vec![node(0), node(0)], vec![node(2), node(0)]] {
        assert!(matches!(
            EvidenceRecord { nodes }.decode_construction(&decoding, &Default::default()),
            Err(DefinitionMappingError::InvalidContract)
        ));
    }
    for depth in [64, 65] {
        let nodes = (0..depth).map(|_| node(1)).chain([node(0)]).collect();
        let bytes = DefaultOptions::new()
            .with_fixint_encoding()
            .serialize(&EvidenceRecord { nodes })
            .unwrap();
        let record: EvidenceRecord = DefaultOptions::new()
            .with_fixint_encoding()
            .deserialize(&bytes)
            .unwrap();
        let decoded = record.decode_construction(&decoding, &Default::default());
        if depth == 64 {
            let decoded = decoded.unwrap();
            let mut ids = Vec::new();
            decoded
                .visit_definition_ids(
                    &mut |id| {
                        ids.push(*id);
                        Ok(())
                    },
                    &Default::default(),
                )
                .unwrap();
            let reencoding = decoding.table.encode(ids).unwrap();
            let record =
                EvidenceRecord::encode_construction(&decoded, &reencoding, &Default::default())
                    .unwrap();
            assert_eq!(
                DefaultOptions::new()
                    .with_fixint_encoding()
                    .serialize(&record)
                    .unwrap(),
                bytes
            );
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
