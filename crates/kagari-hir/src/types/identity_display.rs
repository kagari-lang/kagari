//! Diagnostic names belong to retained scopes; allocation order is not display order.
use crate::types::{NominalType, TypeId};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_types::scalar::BuiltinType;
use std::panic::catch_unwind;

#[test]
fn diagnostic_names_preserve_canonical_order_and_snapshot_ownership() {
    for members in [["Z", "A"], ["A", "Z"]] {
        let context = DefinitionContext::new().unwrap();
        let root = context
            .intern_root(&ModuleIdentity::single_file("display.kgr"))
            .unwrap();
        let owner = context
            .intern_child(root, DefinitionKind::Trait, "View", 0)
            .unwrap();
        let empty = context.snapshot();
        let mut interface = NominalType {
            declaration: owner,
            arguments: vec![TypeId::Builtin(BuiltinType::String)],
            associated_types: Default::default(),
        };
        for member in members {
            let id = context
                .intern_child(owner, DefinitionKind::AssociatedType, member, 0)
                .unwrap();
            interface.associated_types.insert(
                id,
                TypeId::Builtin(if member == "A" {
                    BuiltinType::Bool
                } else {
                    BuiltinType::I32
                }),
            );
        }
        let ty = TypeId::Trait(interface);
        let retained = context.snapshot();
        let expected = "View<String, A = bool, Z = i32>";
        assert_eq!(ty.display_name(&retained), expected);
        assert!(catch_unwind(|| ty.display_name(&empty)).is_err());
        assert!(
            catch_unwind(|| ty.display_name(&DefinitionContext::new().unwrap().snapshot()))
                .is_err()
        );
        let later = context
            .intern_child(root, DefinitionKind::Struct, "Later", 0)
            .unwrap();
        assert!(retained.resolve(later).is_err());
        assert_eq!(ty.display_name(&retained), expected);
        assert_eq!(ty.display_name(&context.snapshot()), expected);
    }
}
