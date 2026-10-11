use crate::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, NativeDeclaration,
        verify::{
            DeclarationValidationError, types_in_scope, types_in_scope_in,
            validate_native_declarations,
        },
    },
    host_interface::host_type_identity,
    scalar::BuiltinType,
    ty::{Constraint, GenericBound, GenericParam, NominalTy, Ty, substitution::TypeTransformError},
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, associated_type_id,
    mapping::{DefinitionMapper, DefinitionRecord},
    metadata::scope_record,
    table::{DefinitionId, DefinitionTableBuilder},
};
use std::{collections::BTreeSet, slice};

fn interface() -> DefinitionPath {
    DefinitionPath {
        module: ModuleIdentity::single_file("scope"),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Output".into(),
            occurrence: 0,
        }],
    }
}

#[test]
fn scoped_validation_preserves_kind_member_and_host_identity_rules() {
    let declaration = interface();
    let member = associated_type_id(&declaration, "Item");
    let mut nominal = NominalTy {
        declaration,
        arguments: vec![],
        associated_types: [(member, Ty::Builtin(BuiltinType::I32))].into(),
    };
    let mut cases = vec![(Ty::Trait(nominal.clone()), true)];
    let (mut member, value) = nominal.associated_types.pop_first().unwrap();
    member.path.last_mut().unwrap().occurrence = 1;
    nominal.associated_types.insert(member, value);
    cases.push((Ty::Trait(nominal), false));
    let host = host_type_identity("game.Entity");
    cases.push((Ty::Host(host.clone()), true));
    let mut wrong_host = host;
    wrong_host.path.last_mut().unwrap().kind = DefinitionKind::Function;
    cases.push((Ty::Host(wrong_host), false));
    let mut wrong_nominal = interface();
    wrong_nominal.path.last_mut().unwrap().kind = DefinitionKind::Struct;
    cases.push((
        Ty::Trait(NominalTy {
            declaration: wrong_nominal,
            arguments: vec![],
            associated_types: Default::default(),
        }),
        false,
    ));
    for (ty, accepted) in cases {
        let cancel = Default::default();
        assert_eq!(types_in_scope([&ty], &[], &cancel), accepted);
        let scoped = scope_record(&ty, &cancel).unwrap();
        assert_eq!(
            types_in_scope_in([scoped.records()], &[], &cancel, Some(scoped.definitions())),
            accepted
        );
        let foreign = DefinitionTableBuilder::new().unwrap().freeze();
        assert!(!types_in_scope_in(
            [scoped.records()],
            &[],
            &cancel,
            Some(&foreign)
        ));
        assert!(!types_in_scope_in([scoped.records()], &[], &cancel, None));
    }
}

#[test]
fn canonical_type_order_ignores_interning_and_associated_member_order() {
    let cancel = Default::default();
    let declaration = interface();
    let first = associated_type_id(&declaration, "A");
    let last = associated_type_id(&declaration, "Z");
    let i32_type = Ty::Builtin(BuiltinType::I32);
    let bool_type = Ty::Builtin(BuiltinType::Bool);
    let nominal = |a, z| NominalTy {
        declaration: declaration.clone(),
        arguments: vec![Ty::Array(Box::new(i32_type.clone()))],
        associated_types: [(first.clone(), a), (last.clone(), z)].into(),
    };
    let cases = vec![
        Ty::Trait(nominal(i32_type.clone(), bool_type.clone())),
        Ty::Trait(nominal(bool_type.clone(), i32_type.clone())),
        Ty::Parameter {
            owner: first.clone(),
            position: 1,
        },
        Ty::Parameter {
            owner: last.clone(),
            position: 0,
        },
        Ty::Function {
            params: vec![bool_type.clone()],
            result: Box::new(i32_type.clone()),
        },
        Ty::Function {
            params: vec![bool_type.clone(), i32_type.clone()],
            result: Box::new(bool_type.clone()),
        },
        Ty::Projection {
            arguments: vec![i32_type.clone()],
            receiver: Box::new(i32_type.clone()),
            interface: Box::new(nominal(bool_type.clone(), bool_type)),
            member: first,
        },
    ];
    let mut paths = BTreeSet::new();
    cases
        .visit_definitions(
            &mut |id| {
                paths.insert(id.clone());
                Ok(())
            },
            &cancel,
        )
        .unwrap();
    let mut builder = DefinitionTableBuilder::new().unwrap();
    for path in paths.iter().rev() {
        builder.intern_path(path).unwrap();
    }
    let table = builder.freeze();
    let scoped = cases
        .map_identities(&mut DefinitionMapper::new(
            &mut |path| Ok(table.lookup(path).unwrap()),
            &cancel,
        ))
        .unwrap();
    for (left_index, left) in cases.iter().enumerate() {
        for (right_index, right) in cases.iter().enumerate() {
            assert_eq!(
                scoped[left_index].canonical_cmp(&scoped[right_index], &table, &cancel),
                Ok(left.cmp(right))
            );
        }
    }
    let foreign = DefinitionTableBuilder::new().unwrap().freeze();
    assert_eq!(
        scoped[0].canonical_cmp(&scoped[4], &foreign, &cancel),
        Err(TypeTransformError::InvalidContract)
    );
    cancel.cancel();
    assert_eq!(
        scoped[0].canonical_cmp(&scoped[0], &table, &cancel),
        Err(TypeTransformError::Cancelled)
    );
}

#[test]
fn native_declarations_validate_scoped_binders_and_canonical_constraints() {
    let cancel = Default::default();
    let context = kagari_common::identity::map::DefinitionContext::new().unwrap();
    let module = ModuleIdentity::single_file("scope");
    let root = context.intern_root(&module).unwrap();
    // Allocate in reverse spelling order; numeric sorting would reject [A, Z].
    let z = context
        .intern_child(root, DefinitionKind::Trait, "Z", 0)
        .unwrap();
    let a = context
        .intern_child(root, DefinitionKind::Trait, "A", 0)
        .unwrap();
    let function = context
        .intern_child(root, DefinitionKind::Function, "consume", 0)
        .unwrap();
    let argument = Ty::Parameter {
        owner: function,
        position: 0,
    };
    let mut declaration = NativeDeclaration {
        declaration: function,
        concrete_result: None,
        callable_requirements: vec![],
        function: FnDecl {
            name: "consume".into(),
            method_policy: Default::default(),
            implementation: CallableImplementation::Native(function),
            generic_params: vec![GenericParam {
                owner: function,
                position: 0,
            }],
            bounds: vec![GenericBound {
                ty: argument.clone(),
                constraints: [a, z]
                    .into_iter()
                    .map(|declaration| {
                        Constraint::Trait(NominalTy {
                            declaration,
                            arguments: vec![],
                            associated_types: Default::default(),
                        })
                    })
                    .collect(),
            }],
            params: vec![],
            return_type: argument,
        },
    };
    let table = context.snapshot();
    let validate = |declaration: &NativeDeclaration<DefinitionId>| {
        validate_native_declarations(slice::from_ref(declaration), &module, &table, &cancel)
    };
    assert_eq!(validate(&declaration), Ok(()));
    declaration.function.bounds[0].constraints.reverse();
    assert_eq!(
        validate(&declaration),
        Err(DeclarationValidationError::Invalid)
    );
    declaration.function.bounds[0].constraints.reverse();
    let mut wrong_owner = declaration.clone();
    wrong_owner.function.generic_params[0].owner = a;
    assert_eq!(
        validate(&wrong_owner),
        Err(DeclarationValidationError::Invalid)
    );
    let mut wrong_name = declaration.clone();
    wrong_name.function.name = "other".into();
    assert_eq!(
        validate(&wrong_name),
        Err(DeclarationValidationError::Invalid)
    );
    let foreign = DefinitionTableBuilder::new().unwrap().freeze();
    assert_eq!(
        validate_native_declarations(slice::from_ref(&declaration), &module, &foreign, &cancel),
        Err(DeclarationValidationError::Invalid)
    );
    cancel.cancel();
    assert_eq!(
        validate(&declaration),
        Err(DeclarationValidationError::Cancelled)
    );
}
