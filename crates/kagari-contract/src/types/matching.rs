//! Match a checked executable interface table through shared semantic rules.
use crate::types::InterfaceTable;
use kagari_common::cancellation::CancellationToken;
use kagari_types::ty::{
    NominalTy, Ty,
    matching::{ImplementationPattern, match_pattern},
    substitution::{TypeSubstitution, TypeTransformError},
};

pub fn match_implementation<'a>(
    table: &'a InterfaceTable,
    interface: &'a NominalTy,
    receiver: &'a Ty,
    cancel: &CancellationToken,
) -> Result<Option<TypeSubstitution<'a>>, TypeTransformError> {
    let Ty::Trait(implemented) = &table.trait_type else {
        return Err(TypeTransformError::InvalidContract);
    };
    match_pattern(
        ImplementationPattern {
            parameters: &table.generic_params,
            receiver: &table.for_type,
            interface: implemented,
        },
        interface,
        receiver,
        cancel,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_common::identity::{
        DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId,
    };
    use kagari_types::{
        collection::CollectionAccess,
        declaration::AssociatedTypeFamily,
        scalar::BuiltinType,
        ty::{GenericParam, matching::projection_output},
    };
    use std::slice;

    fn id(context: &DefinitionContext, kind: DefinitionKind, name: &str) -> DefinitionId {
        let root = context
            .intern_root(&ModuleIdentity::single_file("match.kgr"))
            .unwrap();
        context.intern_child(root, kind, name, 0).unwrap()
    }

    fn applied(declaration: DefinitionId, arguments: Vec<Ty>) -> NominalTy {
        NominalTy {
            declaration,
            arguments,
            associated_types: Default::default(),
        }
    }

    #[test]
    fn implementation_receivers_preserve_exact_access_and_nested_invariance() {
        let context = DefinitionContext::new().unwrap();
        let integer = Ty::Builtin(BuiltinType::I32);
        let parameter = GenericParam {
            owner: id(&context, DefinitionKind::Impl, ""),
            position: 0,
        };
        let custom = id(&context, DefinitionKind::Trait, "CustomSet");
        let interface = applied(custom, vec![integer.clone()]);
        let mut table = InterfaceTable {
            declaration: parameter.owner,
            name: "Set".into(),
            generic_params: vec![parameter.clone()],
            bounds: vec![],
            methods: vec![],
            associated_consts: vec![],
            for_type: Ty::Set(Box::new(parameter.as_type()), CollectionAccess::Mutable),
            trait_type: Ty::Trait(applied(custom, vec![parameter.as_type()])),
            associated_type_families: vec![],
            bridge: None,
        };
        let cancel = CancellationToken::default();
        for declared in [CollectionAccess::Mutable, CollectionAccess::ReadOnly] {
            table.for_type = Ty::Set(Box::new(parameter.as_type()), declared);
            for requested in [CollectionAccess::Mutable, CollectionAccess::ReadOnly] {
                let receiver = Ty::Set(Box::new(integer.clone()), requested);
                assert_eq!(
                    match_implementation(&table, &interface, &receiver, &cancel)
                        .unwrap()
                        .is_some(),
                    declared == requested
                );
            }
        }

        // Nested generic arguments must also keep their exact access descriptor.
        let mutable = Ty::Set(Box::new(integer.clone()), CollectionAccess::Mutable);
        let readonly = Ty::Set(Box::new(integer), CollectionAccess::ReadOnly);
        let interface = applied(custom, vec![readonly.clone()]);
        table.for_type = Ty::Set(Box::new(mutable), CollectionAccess::ReadOnly);
        let nested = Ty::Set(Box::new(readonly), CollectionAccess::ReadOnly);
        assert!(
            match_implementation(&table, &interface, &nested, &cancel)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn implementation_matching_keeps_repeated_binders_consistent_and_applies_families() {
        let context = DefinitionContext::new().unwrap();
        let implementation = id(&context, DefinitionKind::Impl, "");
        let interface_id = id(&context, DefinitionKind::Trait, "Read");
        let parameter = GenericParam {
            owner: implementation,
            position: 0,
        };
        let family_parameter = GenericParam {
            owner: context
                .intern_child(implementation, DefinitionKind::AssociatedType, "Item", 0)
                .unwrap(),
            position: 0,
        };
        let member = context
            .intern_child(interface_id, DefinitionKind::AssociatedType, "Item", 0)
            .unwrap();
        let table = InterfaceTable {
            declaration: implementation,
            name: "Read".into(),
            generic_params: vec![parameter.clone()],
            bounds: vec![],
            methods: vec![],
            associated_consts: vec![],
            for_type: Ty::Tuple(vec![parameter.as_type(), parameter.as_type()]),
            trait_type: Ty::Trait(NominalTy {
                declaration: interface_id,
                arguments: vec![parameter.as_type()],
                associated_types: Default::default(),
            }),
            associated_type_families: vec![AssociatedTypeFamily {
                declaration: member,
                generic_params: vec![family_parameter.clone()],
                bounds: vec![],
                value: Ty::Tuple(vec![parameter.as_type(), family_parameter.as_type()]),
            }],
            bridge: None,
        };
        let integer = Ty::Builtin(BuiltinType::I32);
        let boolean = Ty::Builtin(BuiltinType::Bool);
        let interface = NominalTy {
            declaration: interface_id,
            arguments: vec![integer.clone()],
            associated_types: Default::default(),
        };
        let cancel = CancellationToken::default();
        let receiver = Ty::Tuple(vec![integer.clone(), integer.clone()]);
        assert!(
            match_implementation(&table, &interface, &receiver, &cancel)
                .unwrap()
                .is_some()
        );
        let mismatch = Ty::Tuple(vec![integer.clone(), boolean.clone()]);
        assert!(
            match_implementation(&table, &interface, &mismatch, &cancel)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            projection_output(
                ImplementationPattern {
                    parameters: &table.generic_params,
                    receiver: &table.for_type,
                    interface: match &table.trait_type {
                        Ty::Trait(value) => value,
                        _ => unreachable!(),
                    },
                },
                &table.associated_type_families,
                &interface,
                &receiver,
                &member,
                slice::from_ref(&boolean),
                &cancel
            )
            .unwrap(),
            Some(Ty::Tuple(vec![integer, boolean]))
        );
        cancel.cancel();
        assert!(matches!(
            match_implementation(&table, &interface, &receiver, &cancel),
            Err(TypeTransformError::Cancelled)
        ));
    }
}
