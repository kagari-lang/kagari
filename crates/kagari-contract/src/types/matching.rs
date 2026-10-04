//! Match a checked executable interface table through shared semantic rules.
use crate::types::InterfaceTable;
use kagari_common::cancellation::CancellationToken;
use kagari_types::{
    declaration::TraitDef,
    ty::{
        NominalTy, Ty,
        matching::{ImplementationPattern, match_pattern},
        substitution::{TypeSubstitution, TypeTransformError},
    },
};

pub fn match_implementation<'a>(
    table: &'a InterfaceTable,
    contract: Option<&TraitDef>,
    interface: &'a NominalTy,
    receiver: &'a Ty,
    cancel: &CancellationToken,
) -> Result<Option<TypeSubstitution<'a>>, TypeTransformError> {
    let Ty::Trait(implemented) = &table.trait_type else {
        return Err(TypeTransformError::InvalidContract);
    };
    match_pattern(
        ImplementationPattern {
            storage_access: contract.and_then(|contract| contract.storage_access),
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
    use crate::library;
    use kagari_common::identity::{
        DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, associated_type_id,
    };
    use kagari_types::{
        collection::CollectionAccess,
        declaration::AssociatedTypeFamily,
        scalar::BuiltinType,
        ty::{GenericParam, matching::projection_output},
    };
    use std::slice;

    fn id(kind: DefinitionKind, name: &str) -> DefinitionPath {
        DefinitionPath {
            module: ModuleIdentity::single_file("match.kgr"),
            path: vec![DefinitionPathSegment {
                kind,
                name: name.into(),
                occurrence: 0,
            }],
        }
    }

    #[test]
    fn native_templates_weaken_only_declared_readonly_outer_access() {
        let catalog = crate::library::catalog::shared()
            .into_iter()
            .find(|module| {
                module.identity == crate::library::namespaces::module("std", "collections")
            })
            .unwrap();
        let mut contract = catalog
            .traits
            .iter()
            .find(|contract| contract.name == "List")
            .unwrap()
            .clone();
        let integer = Ty::Builtin(BuiltinType::I32);
        let mutable = Ty::Array(Box::new(integer.clone()), CollectionAccess::Mutable);
        let readonly = Ty::Array(Box::new(integer.clone()), CollectionAccess::ReadOnly);
        let parameter = GenericParam {
            owner: id(DefinitionKind::Impl, ""),
            position: 0,
        };
        let mut interface = library::applied("List", vec![integer.clone()]);
        // An ordinary installed interface gets its capability from its record,
        // without any addition to the language protocol inventory.
        let custom = id(DefinitionKind::Trait, "CustomSequence");
        interface.declaration = custom.clone();
        let mut table = InterfaceTable {
            declaration: parameter.owner.clone(),
            name: "List".into(),
            generic_params: vec![parameter.clone()],
            bounds: vec![],
            methods: vec![],
            associated_consts: vec![],
            for_type: Ty::Array(Box::new(parameter.as_type()), CollectionAccess::Mutable),
            trait_type: Ty::Trait(library::applied("List", vec![parameter.as_type()])),
            associated_type_families: vec![],
            host_bridge: false,
        };
        let Ty::Trait(implemented) = &mut table.trait_type else {
            unreachable!()
        };
        implemented.declaration = custom.clone();
        let cancel = CancellationToken::default();
        assert!(
            match_implementation(&table, Some(&contract), &interface, &mutable, &cancel)
                .unwrap()
                .is_some()
        );
        assert!(
            match_implementation(&table, Some(&contract), &interface, &readonly, &cancel)
                .unwrap()
                .is_some()
        );
        contract.storage_access = Some(CollectionAccess::Mutable);
        let Ty::Trait(implemented) = &mut table.trait_type else {
            unreachable!()
        };
        implemented.declaration = interface.declaration.clone();
        assert!(
            match_implementation(&table, Some(&contract), &interface, &mutable, &cancel)
                .unwrap()
                .is_some()
        );
        assert!(
            match_implementation(&table, Some(&contract), &interface, &readonly, &cancel)
                .unwrap()
                .is_none()
        );
        contract.storage_access = Some(CollectionAccess::ReadOnly);
        // Generic arguments cannot acquire the outer access relaxation.
        interface = library::applied("List", vec![readonly.clone()]);
        interface.declaration = custom.clone();
        table.trait_type = Ty::Trait(library::applied("List", vec![parameter.as_type()]));
        let Ty::Trait(implemented) = &mut table.trait_type else {
            unreachable!()
        };
        implemented.declaration = custom;
        table.for_type = Ty::Array(Box::new(mutable), CollectionAccess::Mutable);
        let nested = Ty::Array(Box::new(readonly), CollectionAccess::ReadOnly);
        assert!(
            match_implementation(&table, Some(&contract), &interface, &nested, &cancel)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn implementation_matching_keeps_repeated_binders_consistent_and_applies_families() {
        let implementation = id(DefinitionKind::Impl, "");
        let interface_id = id(DefinitionKind::Trait, "Read");
        let parameter = GenericParam {
            owner: implementation.clone(),
            position: 0,
        };
        let family_parameter = GenericParam {
            owner: associated_type_id(&implementation, "Item"),
            position: 0,
        };
        let member = associated_type_id(&interface_id, "Item");
        let table = InterfaceTable {
            declaration: implementation,
            name: "Read".into(),
            generic_params: vec![parameter.clone()],
            bounds: vec![],
            methods: vec![],
            associated_consts: vec![],
            for_type: Ty::Tuple(vec![parameter.as_type(), parameter.as_type()]),
            trait_type: Ty::Trait(NominalTy {
                declaration: interface_id.clone(),
                arguments: vec![parameter.as_type()],
                associated_types: Default::default(),
            }),
            associated_type_families: vec![AssociatedTypeFamily {
                declaration: member.clone(),
                generic_params: vec![family_parameter.clone()],
                bounds: vec![],
                value: Ty::Tuple(vec![parameter.as_type(), family_parameter.as_type()]),
            }],
            host_bridge: false,
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
            match_implementation(&table, None, &interface, &receiver, &cancel)
                .unwrap()
                .is_some()
        );
        let mismatch = Ty::Tuple(vec![integer.clone(), boolean.clone()]);
        assert!(
            match_implementation(&table, None, &interface, &mismatch, &cancel)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            projection_output(
                ImplementationPattern {
                    storage_access: None,
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
            match_implementation(&table, None, &interface, &receiver, &cancel),
            Err(TypeTransformError::Cancelled)
        ));
    }
}
