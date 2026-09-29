use super::*;
use crate::{
    callable::CallableImplementation,
    scalar::BuiltinType,
    types::{AssociatedTypeAbi, ConstAbi, FieldAbi, GenericParameterAbi, TypeAbi, TypeAbiKind},
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPathSegment, ModuleIdentity, associated_type_id,
};

fn declaration() -> (DefinitionId, TraitAbi) {
    let id = DefinitionId {
        module: ModuleIdentity::single_file("contracts.kgr"),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Read".into(),
            occurrence: 0,
        }],
    };
    let member = associated_type_id(&id, "Item");
    let record = TraitAbi {
        name: "Read".into(),
        generic_params: vec![GenericParameterAbi {
            owner: id.clone(),
            position: 0,
        }],
        associated_types: vec![AssociatedTypeAbi {
            declaration: member,
            generic_params: vec![],
            parameter_bounds: vec![],
            bounds: vec![],
        }],
        associated_consts: vec![],
        supertraits: vec![],
        bounds: vec![],
        methods: vec![],
    };
    (id, record)
}

#[test]
fn linked_trait_arity_and_members_come_from_the_exact_owner() {
    let (id, record) = declaration();
    let cancel = CancellationToken::default();
    let check = ApplicationValidator::new(&cancel, |key| (key == &id).then_some(&record));
    let mut applied = NominalAbiType {
        declaration: id.clone(),
        arguments: vec![AbiType::Builtin(BuiltinType::I32)],
        associated_types: Default::default(),
    };
    assert_eq!(check.trait_application(&applied), Ok(()));
    applied.arguments.clear();
    assert_eq!(
        check.trait_application(&applied),
        Err(TypeTransformError::InvalidContract)
    );
    applied.arguments.push(AbiType::Builtin(BuiltinType::I32));
    applied.associated_types.insert(
        associated_type_id(&id, "Unknown"),
        AbiType::Builtin(BuiltinType::Bool),
    );
    assert_eq!(
        check.trait_application(&applied),
        Err(TypeTransformError::InvalidContract)
    );
    applied.associated_types.clear();
    applied.declaration.path[0].occurrence = 1;
    assert_eq!(
        check.trait_application(&applied),
        Err(TypeTransformError::InvalidContract)
    );
    applied.declaration = id.clone();
    cancel.cancel();
    assert_eq!(
        check.trait_application(&applied),
        Err(TypeTransformError::Cancelled)
    );
}

#[test]
fn projections_check_trait_arguments_member_identity_and_family_arity() {
    let (id, mut record) = declaration();
    let member = record.associated_types[0].declaration.clone();
    record.associated_types[0]
        .generic_params
        .push(GenericParameterAbi {
            owner: member.clone(),
            position: 0,
        });
    let cancel = CancellationToken::default();
    let check = ApplicationValidator::new(&cancel, |key| (key == &id).then_some(&record));
    let interface = NominalAbiType {
        declaration: id.clone(),
        arguments: vec![AbiType::Builtin(BuiltinType::I32)],
        associated_types: Default::default(),
    };
    let projection = |interface: NominalAbiType, member, arguments| AbiType::Projection {
        receiver: Box::new(AbiType::SelfType(id.clone())),
        interface: Box::new(interface),
        member,
        arguments,
    };
    assert_eq!(
        check.validate_type(&projection(
            interface.clone(),
            member.clone(),
            vec![AbiType::Builtin(BuiltinType::Bool)]
        )),
        Ok(())
    );
    assert_eq!(
        check.validate_type(&projection(interface.clone(), member.clone(), vec![])),
        Err(TypeTransformError::InvalidContract)
    );
    assert_eq!(
        check.validate_type(&projection(
            interface.clone(),
            associated_type_id(&id, "Unknown"),
            vec![]
        )),
        Err(TypeTransformError::InvalidContract)
    );
    let mut invalid = interface;
    invalid
        .associated_types
        .insert(member, AbiType::Builtin(BuiltinType::Bool));
    assert_eq!(
        check.trait_application(&invalid),
        Err(TypeTransformError::InvalidContract)
    );
}

#[test]
fn unused_declarations_and_semantic_slots_cannot_hide_unknown_traits() {
    let (id, _) = declaration();
    let cancel = CancellationToken::default();
    let check = ApplicationValidator::new(&cancel, |_| None);
    let unknown = AbiType::Function {
        params: vec![],
        result: Box::new(AbiType::Tuple(vec![AbiType::Trait(NominalAbiType {
            declaration: id,
            arguments: vec![],
            associated_types: Default::default(),
        })])),
    };
    let items = [
        PublicAbiItem::Const(ConstAbi {
            name: "hidden".into(),
            ty: unknown.clone(),
            value: String::new(),
        }),
        PublicAbiItem::Function(FunctionAbi {
            name: "unused".into(),
            implementation: CallableImplementation::Script,
            generic_params: vec![],
            bounds: vec![],
            params: vec![],
            return_type: unknown.clone(),
        }),
        PublicAbiItem::Type(TypeAbi {
            name: "Unused".into(),
            kind: TypeAbiKind::Struct,
            generic_params: vec![],
            bounds: vec![],
            variants: vec![],
            fields: vec![FieldAbi {
                name: "hidden".into(),
                ty: unknown.clone(),
                mutable: false,
            }],
        }),
    ];
    for item in items {
        assert_eq!(
            check.declarations(&[item], &[]),
            Err(TypeTransformError::InvalidContract)
        );
    }
    let slots = SemanticSlots {
        registers: [(3, unknown)].into(),
        ..Default::default()
    };
    assert_eq!(
        check.slots(&slots),
        Err(TypeTransformError::InvalidContract)
    );
    assert_eq!(
        check.validate_type(&AbiType::Tuple(vec![
            AbiType::Builtin(BuiltinType::Bool);
            MAX_TYPE_NODES
        ])),
        Err(TypeTransformError::LimitExceeded)
    );
}
