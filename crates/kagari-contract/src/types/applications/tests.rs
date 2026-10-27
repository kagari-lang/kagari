use super::*;
use crate::slots::SemanticSlots;
use crate::types::type_contract;
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        AssociatedTypeDef, ConstDef, FieldDef, FnDecl, Param, TypeDef, TypeDefKind,
        native::NativeStorageLayout,
    },
    language::binding,
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty, substitution::MAX_TYPE_NODES},
    visibility::Visibility,
};

fn declaration(context: &DefinitionContext) -> (DefinitionId, TraitDef) {
    let root = context
        .intern_root(&ModuleIdentity::single_file("contracts.kgr"))
        .unwrap();
    let id = context
        .intern_child(root, DefinitionKind::Trait, "Read", 0)
        .unwrap();
    let member = context
        .intern_child(id, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
    let record = TraitDef {
        conversion_adapter: None,
        name: "Read".into(),
        generic_params: vec![GenericParam {
            owner: id,
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
    let context = DefinitionContext::new().unwrap();
    let (id, record) = declaration(&context);
    let cancel = CancellationToken::default();
    let check = ApplicationValidator::new(&cancel, |key| (key == &id).then_some(&record), |_| None);
    let mut applied = NominalTy {
        declaration: id,
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
        context
            .intern_child(id, DefinitionKind::AssociatedType, "Unknown", 0)
            .unwrap(),
        Ty::Builtin(BuiltinType::Bool),
    );
    assert_eq!(
        check.trait_application(&applied),
        Err(TypeTransformError::InvalidContract)
    );
    applied.associated_types.clear();
    applied.declaration = context
        .intern_child(
            context.snapshot().parent(id).unwrap().unwrap(),
            DefinitionKind::Trait,
            "Read",
            1,
        )
        .unwrap();
    assert_eq!(
        check.trait_application(&applied),
        Err(TypeTransformError::InvalidContract)
    );
    applied.declaration = id;
    cancel.cancel();
    assert_eq!(
        check.trait_application(&applied),
        Err(TypeTransformError::Cancelled)
    );
}

#[test]
fn projections_check_trait_arguments_member_identity_and_family_arity() {
    let context = DefinitionContext::new().unwrap();
    let (id, mut record) = declaration(&context);
    let member = record.associated_types[0].declaration;
    record.associated_types[0]
        .generic_params
        .push(GenericParam {
            owner: member,
            position: 0,
        });
    let cancel = CancellationToken::default();
    let check = ApplicationValidator::new(&cancel, |key| (key == &id).then_some(&record), |_| None);
    let interface = NominalTy {
        declaration: id,
        arguments: vec![Ty::Builtin(BuiltinType::I32)],
        associated_types: Default::default(),
    };
    let projection = |interface: NominalTy, member, arguments| Ty::Projection {
        receiver: Box::new(Ty::SelfType(id)),
        interface: Box::new(interface),
        member,
        arguments,
    };
    assert_eq!(
        check.validate_type(&projection(
            interface.clone(),
            member,
            vec![Ty::Builtin(BuiltinType::Bool)]
        )),
        Ok(())
    );
    assert_eq!(
        check.validate_type(&projection(interface.clone(), member, vec![])),
        Err(TypeTransformError::InvalidContract)
    );
    assert_eq!(
        check.validate_type(&projection(
            interface.clone(),
            context
                .intern_child(id, DefinitionKind::AssociatedType, "Unknown", 0)
                .unwrap(),
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
    let context = DefinitionContext::new().unwrap();
    let (id, _) = declaration(&context);
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
            host_policy: None,
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
                host_policy: None,
                visibility: Visibility::Public,
                name: "hidden".into(),
                ty: unknown.clone(),
                mutable: false,
            }],
        }),
    ];
    for item in items {
        assert_eq!(
            validate_declarations(&check, &[item], &[], &[], &[], &cancel, &context.snapshot()),
            Err(TypeTransformError::InvalidContract)
        );
    }
    let slots = SemanticSlots {
        registers: [(3, unknown)].into(),
        ..Default::default()
    };
    assert_eq!(
        validate_slots(&check, &slots, &cancel, &context.snapshot()),
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
    let context = DefinitionContext::new().unwrap();
    let module = ModuleIdentity::single_file("contracts.kgr");
    let root = context.intern_root(&module).unwrap();
    let id = context
        .intern_child(root, DefinitionKind::AssociatedType, "Buffer", 0)
        .unwrap();
    let record = TypeDef {
        name: "Buffer".into(),
        kind: TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
        generic_params: vec![GenericParam {
            owner: id,
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
            type_contract(&module, &items, key, &context.snapshot())
                .map(|record| (record.kind, record.generic_params.len()))
        },
    );
    let mut instance = NominalTy {
        declaration: id,
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
        context
            .intern_child(id, DefinitionKind::AssociatedType, "Item", 0)
            .unwrap(),
        Ty::Builtin(BuiltinType::I32),
    );
    assert_eq!(
        check.validate_type(&Ty::NativeObject(instance.clone())),
        Err(TypeTransformError::InvalidContract)
    );
    instance.associated_types.clear();
    instance.declaration = context
        .intern_child(root, DefinitionKind::AssociatedType, "Buffer", 1)
        .unwrap();
    assert_eq!(
        check.validate_type(&Ty::NativeObject(instance)),
        Err(TypeTransformError::InvalidContract)
    );
}

#[test]
fn ordinary_enum_applications_require_a_declared_owner_and_exact_arity() {
    let context = DefinitionContext::new().unwrap();
    let id = context.intern(&binding::option_declaration()).unwrap();
    let cancel = CancellationToken::default();
    let check = ApplicationValidator::new(
        &cancel,
        |_| None,
        |key| (key == &id).then_some((TypeDefKind::Enum, 1)),
    );
    let applied = |arguments| {
        Ty::Enum(NominalTy {
            declaration: id,
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

#[test]
fn incompatible_traits_remain_static_contracts_but_cannot_hide_in_value_positions() {
    let context = DefinitionContext::new().unwrap();
    let (id, mut record) = declaration(&context);
    record.generic_params.clear();
    record.associated_types.clear();
    context
        .intern_child(id, DefinitionKind::Method, "read", 0)
        .unwrap();
    record.methods.push(FnDecl {
        host_policy: None,
        name: "read".into(),
        implementation: CallableImplementation::Required,
        method_policy: Default::default(),
        generic_params: vec![],
        bounds: vec![],
        params: vec![Param {
            name: "self".into(),
            mutable: false,
            ty: Ty::SelfType(id),
        }],
        return_type: Ty::SelfType(id),
    });
    let cancel = CancellationToken::default();
    let interface = Ty::Trait(NominalTy {
        declaration: id,
        arguments: vec![],
        associated_types: Default::default(),
    });
    for compatible in [false, true] {
        if compatible {
            record.methods[0].return_type = Ty::Builtin(BuiltinType::I32);
        }
        let check =
            ApplicationValidator::new(&cancel, |key| (key == &id).then_some(&record), |_| None);
        assert!(
            validate_declarations(
                &check,
                &[PublicItem::Trait(record.clone())],
                &[],
                &[],
                &[],
                &cancel,
                &context.snapshot()
            )
            .is_ok()
        );
        let nested = Ty::Function {
            params: vec![Ty::Array(Box::new(interface.clone()))],
            result: Box::new(Ty::Builtin(BuiltinType::Unit)),
        };
        let slots = SemanticSlots {
            result: Some(nested.clone()),
            ..Default::default()
        };
        assert_eq!(
            validate_slots(&check, &slots, &cancel, &context.snapshot()).is_ok(),
            compatible
        );
        let field = PublicItem::Type(TypeDef {
            name: "Unused".into(),
            kind: TypeDefKind::Struct,
            generic_params: vec![],
            bounds: vec![],
            variants: vec![],
            fields: vec![FieldDef {
                host_policy: None,
                name: "callback".into(),
                visibility: Visibility::Public,
                mutable: false,
                ty: nested,
            }],
        });
        assert_eq!(
            validate_declarations(
                &check,
                &[field],
                &[],
                &[],
                &[],
                &cancel,
                &context.snapshot()
            )
            .is_ok(),
            compatible
        );
    }
}
