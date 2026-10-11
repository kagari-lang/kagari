use crate::{
    callable::CallableImplementation,
    declaration::{
        AssociatedTypeDef, FnDecl, NativeDeclaration, TraitDef, TypeDef, TypeDefKind,
        module::{ImplDecl, ModuleDecl},
        verify::{
            DeclarationValidationError, Parameters, function_valid, trait_valid, types_in_scope,
            types_in_scope_in, validate_native_declarations,
        },
    },
    host_interface::host_type_identity,
    scalar::BuiltinType,
    ty::{Constraint, GenericBound, GenericParam, NominalTy, Ty, substitution::TypeTransformError},
};
use kagari_common::cancellation::CancellationToken;
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, associated_type_id,
    map::DefinitionContext,
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
    let merged = GenericBound::normalize(
        scoped.iter().rev().map(|ty| GenericBound {
            ty: ty.clone(),
            constraints: vec![],
        }),
        &table,
        &cancel,
    )
    .unwrap();
    let mut order = (0..cases.len()).collect::<Vec<_>>();
    order.sort_by_key(|index| &cases[*index]);
    assert_eq!(
        merged.iter().map(|bound| &bound.ty).collect::<Vec<_>>(),
        order
            .iter()
            .map(|index| &scoped[*index])
            .collect::<Vec<_>>()
    );
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
    let context = DefinitionContext::new().unwrap();
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
        let result =
            validate_native_declarations(slice::from_ref(declaration), &module, &table, &cancel);
        assert_eq!(
            function_valid(
                &declaration.function,
                declaration.declaration,
                &Parameters::new(),
                None,
                &cancel,
                &table
            ),
            result.is_ok()
        );
        result
    };
    assert_eq!(validate(&declaration), Ok(()));
    declaration.function.bounds[0].constraints.reverse();
    assert_eq!(
        validate(&declaration),
        Err(DeclarationValidationError::Invalid)
    );
    let original = declaration.function.bounds[0]
        .constraints
        .iter()
        .rev()
        .cloned()
        .collect::<Vec<_>>();
    declaration.function.bounds = GenericBound::normalize(
        declaration
            .function
            .bounds
            .iter()
            .chain(&declaration.function.bounds)
            .cloned(),
        &table,
        &cancel,
    )
    .unwrap();
    assert_eq!(declaration.function.bounds[0].constraints, original);
    assert_eq!(validate(&declaration), Ok(()));
    let invalid_scope = DefinitionTableBuilder::new().unwrap().freeze();
    assert_eq!(
        GenericBound::normalize(declaration.function.bounds.clone(), &invalid_scope, &cancel),
        Err(TypeTransformError::InvalidContract)
    );
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert_eq!(
        GenericBound::normalize(declaration.function.bounds.clone(), &table, &cancelled),
        Err(TypeTransformError::Cancelled)
    );
    let mut declarations = ModuleDecl::new(module.clone());
    declarations.functions.push(declaration.function.clone());
    assert_eq!(
        declarations.native_declarations(&table).unwrap(),
        vec![declaration.clone()]
    );
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

#[test]
fn trait_members_require_the_retained_scope_and_exact_parent() {
    let cancel = Default::default();
    let context = DefinitionContext::new().unwrap();
    let root = context
        .intern_root(&ModuleIdentity::single_file("traits"))
        .unwrap();
    let owner = context
        .intern_child(root, DefinitionKind::Trait, "Source", 0)
        .unwrap();
    let before = context.snapshot();
    let output = context
        .intern_child(owner, DefinitionKind::AssociatedType, "Output", 0)
        .unwrap();
    context
        .intern_child(owner, DefinitionKind::Method, "read", 0)
        .unwrap();
    let contract = TraitDef {
        name: "Source".into(),
        associated_consts: vec![],
        associated_types: vec![AssociatedTypeDef {
            declaration: output,
            generic_params: vec![],
            parameter_bounds: vec![],
            bounds: vec![],
        }],
        supertraits: vec![],
        generic_params: vec![],
        bounds: vec![],
        methods: vec![FnDecl {
            name: "read".into(),
            implementation: CallableImplementation::Required,
            generic_params: vec![],
            bounds: vec![],
            params: vec![],
            return_type: Ty::Projection {
                receiver: Box::new(Ty::SelfType(owner)),
                interface: Box::new(NominalTy {
                    declaration: owner,
                    arguments: vec![],
                    associated_types: Default::default(),
                }),
                member: output,
                arguments: vec![],
            },
            method_policy: Default::default(),
        }],
        storage_access: None,
        conversion_adapter: None,
    };
    let retained = context.snapshot();
    assert!(trait_valid(&contract, owner, &cancel, &retained));
    let mut module = ModuleDecl::new(retained.resolve(owner).unwrap().module().clone());
    module.traits.push(contract.clone());
    assert!(module.validate(&|_| None, &retained).is_ok());
    assert!(module.validate(&|_| None, &before).is_err());
    let binding = context
        .intern_child(root, DefinitionKind::Function, "read_binding", 0)
        .unwrap();
    module
        .implement_trait(
            &contract,
            ImplDecl {
                generic_params: vec![],
                bounds: vec![],
                trait_type: Some(NominalTy {
                    declaration: owner,
                    arguments: vec![],
                    associated_types: [(output, Ty::Builtin(BuiltinType::I32))].into(),
                }),
                for_type: Ty::Builtin(BuiltinType::I32),
                methods: vec![],
            },
            &[("read", binding)],
            &context,
        )
        .unwrap();
    assert_eq!(
        module.implementations[0].methods[0].return_type,
        Ty::Builtin(BuiltinType::I32)
    );
    let implemented = context.snapshot();
    assert!(module.validate(&|_| None, &implemented).is_ok());
    assert!(module.validate(&|_| None, &retained).is_err());
    let mut changed = module.clone();
    changed.implementations[0].methods[0].return_type = Ty::Builtin(BuiltinType::Bool);
    assert!(changed.validate(&|_| None, &implemented).is_err());
    let foreign_context = DefinitionContext::new().unwrap();
    let count = foreign_context.snapshot().len();
    assert!(module.collect_definitions(&foreign_context).is_err());
    assert_eq!(foreign_context.snapshot().len(), count);
    let mut oversized = ModuleDecl::new(module.identity.clone());
    oversized.types = (0..4097)
        .map(|index| TypeDef {
            name: format!("Type{index}"),
            kind: TypeDefKind::Enum,
            generic_params: vec![],
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        })
        .collect();
    assert!(oversized.collect_definitions(&foreign_context).is_err());
    assert_eq!(foreign_context.snapshot().len(), count);

    assert!(!trait_valid(&contract, owner, &cancel, &before));
    let foreign = DefinitionContext::new().unwrap();
    assert!(!trait_valid(&contract, owner, &cancel, &foreign.snapshot()));
    let other = context
        .intern_child(root, DefinitionKind::Trait, "Other", 0)
        .unwrap();
    let other_output = context
        .intern_child(other, DefinitionKind::AssociatedType, "Output", 0)
        .unwrap();
    let mut invalid = contract.clone();
    invalid.associated_types[0].declaration = other_output;
    assert!(!trait_valid(&invalid, owner, &cancel, &context.snapshot()));
    assert!(trait_valid(&contract, owner, &cancel, &retained));
}
