use super::*;
use crate::{
    layout::EnumVariantLayout,
    scalar::BuiltinType,
    standard::{intrinsic, surface::StandardEnum, traits::StandardTrait},
    types::{AssociatedTypeFamilyAbi, GenericParameterAbi},
};
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionKind, DefinitionPathSegment, ModuleIdentity, associated_type_id},
};

fn id(kind: DefinitionKind, name: &str) -> DefinitionId {
    DefinitionId {
        module: ModuleIdentity::single_file("proof.kgr"),
        path: vec![DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    }
}
fn nominal(id: DefinitionId, arguments: Vec<AbiType>) -> NominalAbiType {
    NominalAbiType {
        declaration: id,
        arguments,
        associated_types: BTreeMap::new(),
    }
}
fn scalar() -> AbiType {
    AbiType::Builtin(BuiltinType::I32)
}
fn table(name: &str, interface: NominalAbiType, receiver: AbiType) -> InterfaceTableAbi {
    InterfaceTableAbi {
        declaration: id(DefinitionKind::Impl, name),
        name: name.into(),
        generic_params: vec![],
        bounds: vec![],
        trait_type: AbiType::Trait(interface),
        for_type: receiver,
        methods: vec![],
        associated_consts: vec![],
        associated_type_families: vec![],
        host_bridge: false,
        native_bridge: false,
    }
}
fn bound(ty: AbiType, required: NominalAbiType) -> GenericBoundAbi {
    GenericBoundAbi {
        ty,
        constraints: vec![ConstraintAbi::Trait(required)],
    }
}

#[test]
fn linked_proofs_discharge_generic_bounds_and_reject_ambiguity_and_cycles() {
    let cancel = CancellationToken::default();
    let marker = nominal(id(DefinitionKind::Trait, "Marker"), vec![]);
    let mut generic = table("generic", marker.clone(), scalar());
    let parameter = GenericParameterAbi {
        owner: generic.declaration.clone(),
        position: 0,
    };
    generic.for_type = AbiType::Array(Box::new(parameter.as_type()), CollectionAccess::Mutable);
    generic.generic_params.push(parameter.clone());
    generic.bounds.push(bound(
        parameter.as_type(),
        intrinsic::applied(StandardTrait::Eq, vec![]),
    ));
    let query = |item| AbiType::Array(Box::new(item), CollectionAccess::Mutable);
    let catalog = ProofCatalog::new(vec![&generic], vec![], [], [], &cancel).unwrap();
    assert!(
        catalog
            .holds(&marker, &query(scalar()), &[], &cancel)
            .unwrap()
    );
    assert!(
        !catalog
            .holds(
                &marker,
                &query(AbiType::Builtin(BuiltinType::F32)),
                &[],
                &cancel
            )
            .unwrap()
    );
    let another = table("concrete", marker.clone(), query(scalar()));
    let catalog = ProofCatalog::new(vec![&generic, &another], vec![], [], [], &cancel).unwrap();
    assert_eq!(
        catalog
            .implementation_count(&marker, &query(scalar()), &[], &cancel)
            .unwrap(),
        2
    );
    assert!(
        !catalog
            .holds(&marker, &query(scalar()), &[], &cancel)
            .unwrap()
    );
    let mut recursive = table("recursive", marker.clone(), scalar());
    recursive.bounds.push(bound(scalar(), marker.clone()));
    let catalog = ProofCatalog::new(vec![&recursive], vec![], [], [], &cancel).unwrap();
    assert!(!catalog.holds(&marker, &scalar(), &[], &cancel).unwrap());
    cancel.cancel();
    assert_eq!(
        catalog.holds(&marker, &scalar(), &[], &cancel),
        Err(TypeTransformError::Cancelled)
    );
}

#[test]
fn nominal_equality_overrides_disable_defaults_and_require_owned_explicit_prerequisites() {
    let cancel = CancellationToken::default();
    let receiver = AbiType::Struct(nominal(id(DefinitionKind::Struct, "Key"), vec![]));
    let partial = table(
        "partial",
        intrinsic::applied(StandardTrait::PartialEq, vec![]),
        receiver.clone(),
    );
    let eq = table(
        "eq",
        intrinsic::applied(StandardTrait::Eq, vec![]),
        receiver.clone(),
    );
    let hash = table(
        "hash",
        intrinsic::applied(StandardTrait::Hash, vec![]),
        receiver.clone(),
    );
    let catalog = ProofCatalog::new(vec![&partial], vec![], [], [], &cancel).unwrap();
    assert!(catalog.overrides_valid(&cancel).unwrap());
    assert!(
        !catalog
            .holds(
                &intrinsic::applied(StandardTrait::Eq, vec![]),
                &receiver,
                &[],
                &cancel
            )
            .unwrap()
    );
    let catalog = ProofCatalog::new(vec![&partial, &hash], vec![], [], [], &cancel).unwrap();
    assert!(!catalog.overrides_valid(&cancel).unwrap());
    let catalog = ProofCatalog::new(vec![&partial, &eq, &hash], vec![], [], [], &cancel).unwrap();
    assert!(catalog.overrides_valid(&cancel).unwrap());
    let mut foreign = partial.clone();
    foreign.declaration.module = ModuleIdentity::single_file("foreign.kgr");
    assert!(
        !ProofCatalog::new(vec![&foreign], vec![], [], [], &cancel)
            .unwrap()
            .overrides_valid(&cancel)
            .unwrap()
    );
    assert!(ProofCatalog::new(vec![&partial, &partial], vec![], [], [], &cancel).is_err());
}

#[test]
fn family_projection_normalization_applies_both_binders_and_rejects_cycles() {
    let cancel = CancellationToken::default();
    let interface = nominal(id(DefinitionKind::Trait, "Project"), vec![]);
    let member = associated_type_id(&interface.declaration, "Item");
    let mut implementation = table("family", interface.clone(), scalar());
    let parameter = GenericParameterAbi {
        owner: implementation.declaration.clone(),
        position: 0,
    };
    implementation.generic_params.push(parameter.clone());
    implementation.for_type =
        AbiType::Array(Box::new(parameter.as_type()), CollectionAccess::Mutable);
    let input = GenericParameterAbi {
        owner: member.clone(),
        position: 0,
    };
    implementation
        .associated_type_families
        .push(AssociatedTypeFamilyAbi {
            declaration: member.clone(),
            generic_params: vec![input.clone()],
            bounds: vec![],
            value: AbiType::Tuple(vec![parameter.as_type(), input.as_type()]),
        });
    let projection = AbiType::Projection {
        receiver: Box::new(AbiType::Array(
            Box::new(scalar()),
            CollectionAccess::Mutable,
        )),
        interface: Box::new(interface.clone()),
        member: member.clone(),
        arguments: vec![AbiType::Builtin(BuiltinType::Bool)],
    };
    let catalog = ProofCatalog::new(vec![&implementation], vec![], [], [], &cancel).unwrap();
    assert_eq!(
        catalog.normalize(&projection, &cancel).unwrap(),
        AbiType::Tuple(vec![scalar(), AbiType::Builtin(BuiltinType::Bool)])
    );
    implementation.associated_type_families[0].value = projection.clone();
    let catalog = ProofCatalog::new(vec![&implementation], vec![], [], [], &cancel).unwrap();
    assert!(catalog.normalize(&projection, &cancel).is_err());
}

#[test]
fn structural_enum_defaults_are_coinductive_but_reject_non_hashable_payloads() {
    let cancel = CancellationToken::default();
    let instance = nominal(id(DefinitionKind::Enum, "Chain"), vec![]);
    let ty = AbiType::Enum(instance.clone());
    let mut layout = EnumLayout {
        declaration: instance.declaration,
        arguments: vec![],
        variants: vec![EnumVariantLayout {
            declaration: id(DefinitionKind::Variant, "Next"),
            payload: vec![ty.clone()],
        }],
    };
    let catalog = ProofCatalog::new(vec![], vec![], [&layout], [], &cancel).unwrap();
    assert!(
        catalog
            .holds(
                &intrinsic::applied(StandardTrait::Hash, vec![]),
                &ty,
                &[],
                &cancel
            )
            .unwrap()
    );
    layout.variants[0]
        .payload
        .push(AbiType::Builtin(BuiltinType::F64));
    let catalog = ProofCatalog::new(vec![], vec![], [&layout], [], &cancel).unwrap();
    assert!(
        !catalog
            .holds(
                &intrinsic::applied(StandardTrait::Hash, vec![]),
                &ty,
                &[],
                &cancel
            )
            .unwrap()
    );
    let set = AbiType::Set(Box::new(ty), CollectionAccess::Mutable);
    let collect = intrinsic::applied(
        StandardTrait::FromIterator,
        vec![AbiType::Enum(nominal(layout.declaration.clone(), vec![]))],
    );
    assert!(!catalog.holds(&collect, &set, &[], &cancel).unwrap());
    // Storage itself remains an identity key regardless of element equality.
    let wrapped = AbiType::StandardEnum {
        kind: StandardEnum::Option,
        args: vec![set],
    };
    assert!(
        catalog
            .holds(
                &intrinsic::applied(StandardTrait::Hash, vec![]),
                &wrapped,
                &[],
                &cancel
            )
            .unwrap()
    );
}

#[test]
fn recursive_growth_is_bounded_and_host_candidates_participate_in_uniqueness() {
    use kagari_common::host_interface::{HostAssociatedTypeBinding, HostValueType};
    let cancel = CancellationToken::default();
    let marker = nominal(id(DefinitionKind::Trait, "Marker"), vec![]);
    let mut growing = table("growing", marker.clone(), scalar());
    let parameter = GenericParameterAbi {
        owner: growing.declaration.clone(),
        position: 0,
    };
    growing.for_type = parameter.as_type();
    growing.generic_params.push(parameter.clone());
    growing.bounds.push(bound(
        AbiType::Array(Box::new(parameter.as_type()), CollectionAccess::Mutable),
        marker.clone(),
    ));
    let catalog = ProofCatalog::new(vec![&growing], vec![], [], [], &cancel).unwrap();
    assert_eq!(
        catalog.holds(&marker, &scalar(), &[], &cancel),
        Err(TypeTransformError::LimitExceeded)
    );
    let mut host = HostTypeDeclaration::new("proof_host");
    let mut implementation =
        HostTraitImplementationDeclaration::new(marker.declaration.clone(), vec![], vec![]);
    let member = associated_type_id(&marker.declaration, "Item");
    implementation
        .associated_types
        .push(HostAssociatedTypeBinding {
            declaration: member.clone(),
            ty: HostValueType::I32,
        });
    host.trait_implementations.push(implementation);
    let receiver = AbiType::Host(host.id.clone());
    let catalog = ProofCatalog::new(vec![], vec![&host], [], [], &cancel).unwrap();
    assert_eq!(
        catalog
            .implementation_count(&marker, &receiver, &[], &cancel)
            .unwrap(),
        1
    );
    let projection = AbiType::Projection {
        receiver: Box::new(receiver.clone()),
        interface: Box::new(marker.clone()),
        member,
        arguments: vec![],
    };
    assert_eq!(catalog.normalize(&projection, &cancel).unwrap(), scalar());
    let script = table(
        "host_script",
        host_application(&host.trait_implementations[0]),
        receiver.clone(),
    );
    let catalog = ProofCatalog::new(vec![&script], vec![&host], [], [], &cancel).unwrap();
    assert_eq!(
        catalog
            .implementation_count(&marker, &receiver, &[], &cancel)
            .unwrap(),
        2
    );
    assert_eq!(catalog.normalize(&projection, &cancel).unwrap(), projection);
}
