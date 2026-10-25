use crate::{
    slots::SemanticSlots,
    types::{
        ConcreteFunctionIdentity, InterfaceTable,
        named::{
            NamedContract, NamedDeclaration,
            verify::{SignatureScope, signature_matches},
        },
        proofs::ProofCatalog,
    },
};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_types::{
    callable::CallableImplementation,
    declaration::FnDecl,
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
    visibility::Visibility,
};
use std::slice;

#[test]
fn applied_named_projection_requires_the_callers_linked_proof() {
    let context = DefinitionContext::new().unwrap();
    let root = context
        .intern_root(&ModuleIdentity::single_file("projection-signature"))
        .unwrap();
    let function = context
        .intern_child(root, DefinitionKind::Function, "read", 0)
        .unwrap();
    let interface = context
        .intern_child(root, DefinitionKind::Trait, "Read", 0)
        .unwrap();
    let output = context
        .intern_child(interface, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
    let selected = context
        .intern_child(root, DefinitionKind::Impl, "", 0)
        .unwrap();
    let nominal = NominalTy {
        declaration: interface,
        arguments: vec![],
        associated_types: Default::default(),
    };
    let parameter = GenericParam {
        owner: function,
        position: 0,
    };
    let record = NamedDeclaration {
        declaration: function,
        visibility: Visibility::Private,
        ambiguous_local_body: None,
        contract: NamedContract::Function(FnDecl {
            name: "read".into(),
            implementation: CallableImplementation::Script,
            host_policy: None,
            method_policy: Default::default(),
            generic_params: vec![parameter.clone()],
            bounds: vec![],
            params: vec![],
            return_type: Ty::Projection {
                receiver: Box::new(parameter.as_type()),
                interface: Box::new(nominal.clone()),
                member: output,
                arguments: vec![],
            },
        }),
    };
    let mut applied = nominal;
    applied
        .associated_types
        .insert(output, Ty::Builtin(BuiltinType::I32));
    let implementation = InterfaceTable {
        declaration: selected,
        name: "Read for i32".into(),
        generic_params: vec![],
        bounds: vec![],
        trait_type: Ty::Trait(applied),
        for_type: Ty::Builtin(BuiltinType::I32),
        methods: vec![],
        associated_consts: vec![],
        associated_type_families: vec![],
        bridge: None,
    };
    let identity = ConcreteFunctionIdentity {
        declaration: function,
        arguments: vec![Ty::Builtin(BuiltinType::I32)],
    };
    let definitions = context.snapshot();
    let cancel = Default::default();
    let catalog = ProofCatalog::new(
        vec![(&implementation).into()],
        [],
        [],
        [],
        &definitions,
        &cancel,
    )
    .unwrap();
    let unavailable = ProofCatalog::new(vec![], [], [], [], &definitions, &cancel).unwrap();
    let mut semantic = SemanticSlots {
        result: Some(Ty::Builtin(BuiltinType::I32)),
        ..Default::default()
    };
    let matches = |scope, semantic: &SemanticSlots| {
        signature_matches(
            scope,
            slice::from_ref(&record),
            &identity,
            semantic,
            &definitions,
            &cancel,
        )
    };
    assert!(matches(SignatureScope::Local, &semantic));
    assert!(matches(SignatureScope::Linked(&catalog), &semantic));
    assert!(!matches(SignatureScope::Linked(&unavailable), &semantic));
    semantic.result = Some(Ty::Builtin(BuiltinType::Bool));
    assert!(matches(SignatureScope::Local, &semantic));
    assert!(!matches(SignatureScope::Linked(&catalog), &semantic));
}
