use super::*;
use crate::types::type_contract;
use kagari_common::identity::{
    DefinitionKind, DefinitionPathSegment, ModuleIdentity, associated_type_id,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        AssociatedTypeDef, ConstDef, FieldDef, FnDecl, TypeDef, TypeDefKind,
        native::NativeStorageLayout,
    },
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty, substitution::MAX_TYPE_NODES},
};

fn declaration() -> (DefinitionPath, TraitDef) {
    let id = DefinitionPath {
        module: ModuleIdentity::single_file("contracts.kgr"),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Read".into(),
            occurrence: 0,
        }],
    };
    let member = associated_type_id(&id, "Item");
    let record = TraitDef {
        conversion_adapter: None,
        storage_access: None,
        name: "Read".into(),
        generic_params: vec![GenericParam {
            owner: id.clone(),
            position: 0,
        }],
        associated_types: vec![AssociatedTypeDef {
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
    let check = ApplicationValidator::new(&cancel, |key| (key == &id).then_some(&record), |_| None);
    let mut applied = NominalTy {
        declaration: id.clone(),
        arguments: vec![Ty::Builtin(BuiltinType::I32)],
        associated_types: Default::default(),
    };
    assert_eq!(check.trait_application(&applied), Ok(()));
    applied.arguments.clear();
    assert_eq!(
        check.trait_application(&applied),
        Err(TypeTransformError::InvalidContract)
    );
    applied.arguments.push(Ty::Builtin(BuiltinType::I32));
    applied.associated_types.insert(
        associated_type_id(&id, "Unknown"),
        Ty::Builtin(BuiltinType::Bool),
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
        .push(GenericParam {
            owner: member.clone(),
            position: 0,
        });
    let cancel = CancellationToken::default();
    let check = ApplicationValidator::new(&cancel, |key| (key == &id).then_some(&record), |_| None);
    let interface = NominalTy {
        declaration: id.clone(),
        arguments: vec![Ty::Builtin(BuiltinType::I32)],
        associated_types: Default::default(),
    };
    let projection = |interface: NominalTy, member, arguments| Ty::Projection {
        receiver: Box::new(Ty::SelfType(id.clone())),
        interface: Box::new(interface),
        member,
        arguments,
    };
    assert_eq!(
        check.validate_type(&projection(
            interface.clone(),
            member.clone(),
            vec![Ty::Builtin(BuiltinType::Bool)]
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
        .insert(member, Ty::Builtin(BuiltinType::Bool));
    assert_eq!(
        check.trait_application(&invalid),
        Err(TypeTransformError::InvalidContract)
    );
}

#[test]
fn unused_declarations_and_semantic_slots_cannot_hide_unknown_traits() {
    let (id, _) = declaration();
    let cancel = CancellationToken::default();
    let check = ApplicationValidator::new(&cancel, |_| None, |_| None);
    let unknown = Ty::Function {
        params: vec![],
        result: Box::new(Ty::Tuple(vec![Ty::Trait(NominalTy {
            declaration: id,
            arguments: vec![],
            associated_types: Default::default(),
        })])),
    };
    let items = [
        PublicItem::Const(ConstDef {
            name: "hidden".into(),
            ty: unknown.clone(),
            value: String::new(),
        }),
        PublicItem::Function(FnDecl {
            method_policy: Default::default(),
            name: "unused".into(),
            implementation: CallableImplementation::Script,
            generic_params: vec![],
            bounds: vec![],
            params: vec![],
            return_type: unknown.clone(),
        }),
        PublicItem::Type(TypeDef {
            name: "Unused".into(),
            kind: TypeDefKind::Struct,
            generic_params: vec![],
            bounds: vec![],
            variants: vec![],
            fields: vec![FieldDef {
                name: "hidden".into(),
                ty: unknown.clone(),
                mutable: false,
            }],
        }),
    ];
    for item in items {
        assert_eq!(
            validate_declarations(&check, &[item], &[], &cancel),
            Err(TypeTransformError::InvalidContract)
        );
    }
    let slots = SemanticSlots {
        registers: [(3, unknown)].into(),
        ..Default::default()
    };
    assert_eq!(
        validate_slots(&check, &slots, &cancel),
        Err(TypeTransformError::InvalidContract)
    );
    assert_eq!(
        check.validate_type(&Ty::Tuple(vec![
            Ty::Builtin(BuiltinType::Bool);
            MAX_TYPE_NODES
        ])),
        Err(TypeTransformError::LimitExceeded)
    );
}

#[test]
fn native_storage_applications_require_the_actual_owner_and_layout_arity() {
    let (mut id, _) = declaration();
    id.path[0].kind = DefinitionKind::AssociatedType;
    id.path[0].name = "Buffer".into();
    let record = TypeDef {
        name: "Buffer".into(),
        kind: TypeDefKind::NativeStorage(NativeStorageLayout::Sequence { element: 0 }),
        generic_params: vec![GenericParam {
            owner: id.clone(),
            position: 0,
        }],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    };
    let items = [PublicItem::Type(record)];
    let cancel = CancellationToken::default();
    let check = ApplicationValidator::new(
        &cancel,
        |_| None,
        |key| {
            type_contract(&id.module, &items, key)
                .map(|record| (record.kind, record.generic_params.len()))
        },
    );
    let mut instance = NominalTy {
        declaration: id.clone(),
        arguments: vec![Ty::Builtin(BuiltinType::I32)],
        associated_types: Default::default(),
    };
    assert_eq!(
        check.validate_type(&Ty::NativeObject(instance.clone())),
        Ok(())
    );
    instance.arguments.clear();
    assert_eq!(
        check.validate_type(&Ty::NativeObject(instance.clone())),
        Err(TypeTransformError::InvalidContract)
    );
    instance.arguments.push(Ty::Builtin(BuiltinType::I32));
    instance.associated_types.insert(
        associated_type_id(&id, "Item"),
        Ty::Builtin(BuiltinType::I32),
    );
    assert_eq!(
        check.validate_type(&Ty::NativeObject(instance.clone())),
        Err(TypeTransformError::InvalidContract)
    );
    instance.associated_types.clear();
    instance.declaration.path[0].occurrence = 1;
    assert_eq!(
        check.validate_type(&Ty::NativeObject(instance)),
        Err(TypeTransformError::InvalidContract)
    );
}

#[test]
fn ordinary_enum_applications_require_a_declared_owner_and_exact_arity() {
    use kagari_types::declaration::TypeDefKind;
    let id = kagari_types::language::binding::option_declaration();
    let cancel = CancellationToken::default();
    let check = ApplicationValidator::new(
        &cancel,
        |_| None,
        |key| (key == &id).then_some((TypeDefKind::Enum, 1)),
    );
    let applied = |arguments| {
        Ty::Enum(NominalTy {
            declaration: id.clone(),
            arguments,
            associated_types: Default::default(),
        })
    };
    assert!(
        check
            .validate_type(&applied(vec![Ty::Builtin(BuiltinType::I32)]))
            .is_ok()
    );
    for arguments in [
        vec![],
        vec![
            Ty::Builtin(BuiltinType::I32),
            Ty::Builtin(BuiltinType::Bool),
        ],
    ] {
        assert_eq!(
            check.validate_type(&applied(arguments)),
            Err(TypeTransformError::InvalidContract)
        );
    }
    let missing = ApplicationValidator::new(&cancel, |_| None, |_| None);
    assert_eq!(
        missing.validate_type(&applied(vec![Ty::Builtin(BuiltinType::I32)])),
        Err(TypeTransformError::InvalidContract)
    );
    let wrong = ApplicationValidator::new(
        &cancel,
        |_| None,
        |key| (key == &id).then_some((TypeDefKind::Struct, 1)),
    );
    assert_eq!(
        wrong.validate_type(&applied(vec![Ty::Builtin(BuiltinType::I32)])),
        Err(TypeTransformError::InvalidContract)
    );
}
