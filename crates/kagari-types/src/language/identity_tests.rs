//! Protocol recognition preserves canonical ownership across scoped snapshots.
use crate::language::{Protocol, identity};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext};

#[test]
fn protocol_lookup_requires_exact_identity_in_the_retained_scope() {
    let context = DefinitionContext::new().unwrap();
    let before = context.snapshot();
    for protocol in Protocol::ALL {
        let root = context.intern_root(&protocol.module()).unwrap();
        let wrong_kind = context
            .intern_child(root, DefinitionKind::Struct, protocol.name(), 0)
            .unwrap();
        let duplicate = context
            .intern_child(root, DefinitionKind::Trait, protocol.name(), 1)
            .unwrap();
        let foreign_root = context
            .intern_root(&ModuleIdentity {
                package: PackageId("application".into()),
                path: protocol.module().path,
            })
            .unwrap();
        let foreign = context
            .intern_child(foreign_root, DefinitionKind::Trait, protocol.name(), 0)
            .unwrap();
        let missing = context.snapshot();
        assert_eq!(protocol.definition(&missing), None);
        for impostor in [wrong_kind, duplicate, foreign] {
            assert_eq!(Protocol::from_definition(&impostor, &missing), None);
        }

        let declaration = context.intern(&identity(protocol)).unwrap();
        let installed = context.snapshot();
        assert_eq!(protocol.definition(&installed), Some(declaration));
        assert_eq!(
            Protocol::from_definition(&declaration, &installed),
            Some(protocol)
        );
        assert_eq!(protocol.definition(&missing), None);
        assert_eq!(Protocol::from_definition(&declaration, &before), None);

        let adopted = DefinitionContext::new().unwrap();
        adopted.intern(&identity(Protocol::Try)).unwrap();
        let remap = adopted.import(&installed, [declaration]).unwrap();
        let adopted_id = remap.map(declaration).unwrap();
        let adopted_table = adopted.snapshot();
        assert_ne!(adopted_id, declaration);
        assert_eq!(protocol.definition(&adopted_table), Some(adopted_id));
        assert_eq!(
            Protocol::from_definition(&adopted_id, &adopted_table),
            Some(protocol)
        );
        assert_eq!(
            Protocol::from_definition(&declaration, &adopted_table),
            None
        );
    }
}
