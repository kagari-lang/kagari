use crate::types::{
    InterfaceTable, PublicItem,
    inherent::{InherentTable, native_signatures_match},
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, NativeDeclaration, Param},
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
};
use std::collections::BTreeMap;
fn owner(context: &DefinitionContext, name: &str) -> DefinitionId {
    let root = context
        .intern_root(&ModuleIdentity::single_file("substitution.kgr"))
        .unwrap();
    context
        .intern_child(root, DefinitionKind::Trait, name, 0)
        .unwrap()
}

fn parameter(owner: &DefinitionId, position: usize) -> Ty {
    Ty::Parameter {
        owner: *owner,
        position,
    }
}

#[test]
fn impl_instantiation_preserves_method_generics_and_substitutes_their_bounds() {
    let context = DefinitionContext::new().unwrap();
    let implementation = owner(&context, "Impl");
    let method = context
        .intern_child(
            implementation,
            DefinitionKind::AssociatedType,
            "construct",
            0,
        )
        .unwrap();
    let interface = owner(&context, "Iterable");
    let item = context
        .intern_child(interface, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
    let outer = GenericParam {
        owner: implementation,
        position: 0,
    };
    let local = GenericParam {
        owner: method,
        position: 0,
    };
    let source = parameter(&method, 0);
    let input = parameter(&implementation, 0);
    let table = InterfaceTable {
        associated_type_families: vec![],
        associated_consts: vec![],
        bridge: None,
        declaration: implementation,
        name: "Impl".into(),
        generic_params: vec![outer.clone()],
        bounds: vec![],
        trait_type: Ty::Trait(NominalTy {
            declaration: interface,
            arguments: vec![],
            associated_types: BTreeMap::new(),
        }),
        for_type: Ty::Tuple(vec![input.clone()]),
        methods: vec![FnDecl {
            host_policy: None,
            method_policy: Default::default(),
            name: "construct".into(),
            implementation: CallableImplementation::Script,
            generic_params: vec![local.clone()],
            bounds: vec![GenericBound {
                ty: source.clone(),
                constraints: vec![NominalTy {
                    declaration: interface,
                    arguments: vec![],
                    associated_types: BTreeMap::from([(item, input.clone())]),
                }],
            }],
            params: vec![Param {
                name: "source".into(),
                mutable: false,
                ty: source.clone(),
            }],
            return_type: Ty::Tuple(vec![input]),
        }],
    };
    let number = Ty::Builtin(BuiltinType::I32);
    let applied = table
        .instantiate(std::slice::from_ref(&number), &[], &context.snapshot())
        .unwrap();
    assert_eq!(applied.for_type, Ty::Tuple(vec![number.clone()]));
    assert!(applied.generic_params.is_empty());
    let method = &applied.methods[0];
    assert_eq!(method.generic_params, [local]);
    assert_eq!(method.params[0].ty, source);
    assert_eq!(method.return_type, Ty::Tuple(vec![number.clone()]));
    assert_eq!(method.bounds[0].ty, source);
    let bound = &method.bounds[0].constraints[0];
    assert_eq!(bound.associated_types[&item], number);
}

#[test]
fn native_inherent_bounds_merge_canonically_without_weakening_the_installed_contract() {
    let context = DefinitionContext::new().unwrap();
    let root = context
        .intern_root(&ModuleIdentity::single_file("native-inherent.kgr"))
        .unwrap();
    let implementation = context
        .intern_child(root, DefinitionKind::Impl, "Array", 0)
        .unwrap();
    let method = context
        .intern_child(implementation, DefinitionKind::Method, "map", 0)
        .unwrap();
    let outer = GenericParam {
        owner: implementation,
        position: 0,
    };
    let local = GenericParam {
        owner: method,
        position: 0,
    };
    // Allocate in reverse semantic order to distinguish identities from slots.
    let omega = owner(&context, "Omega");
    let alpha = owner(&context, "Alpha");
    let output = context
        .intern_child(alpha, DefinitionKind::AssociatedType, "Output", 0)
        .unwrap();
    let omega = NominalTy {
        declaration: omega,
        arguments: vec![],
        associated_types: BTreeMap::new(),
    };
    let alpha = NominalTy {
        declaration: alpha,
        arguments: vec![local.as_type()],
        associated_types: BTreeMap::from([(output, outer.as_type())]),
    };
    let receiver = Ty::Array(Box::new(outer.as_type()));
    let function = FnDecl {
        host_policy: None,
        method_policy: Default::default(),
        name: "map".into(),
        implementation: CallableImplementation::Native(method),
        generic_params: vec![local.clone()],
        bounds: vec![GenericBound {
            ty: outer.as_type(),
            constraints: vec![omega.clone(), alpha.clone()],
        }],
        params: vec![Param {
            name: "self".into(),
            ty: receiver.clone(),
            mutable: false,
        }],
        return_type: local.as_type(),
    };
    let items = [PublicItem::InherentTable(Box::new(InherentTable {
        declaration: implementation,
        name: "Array".into(),
        generic_params: vec![outer.clone()],
        bounds: vec![GenericBound {
            ty: outer.as_type(),
            constraints: vec![omega.clone()],
        }],
        for_type: receiver,
        methods: vec![function.clone()],
    }))];
    let mut installed = NativeDeclaration {
        declaration: method,
        function,
        callable_requirements: vec![],
        concrete_result: None,
    };
    installed.function.generic_params = vec![outer, local];
    installed.function.bounds[0].constraints = vec![alpha, omega.clone()];
    let matches =
        |declaration| native_signatures_match(&items, &[declaration], &context.snapshot());
    assert!(matches(installed.clone()));
    for mutation in 0..8 {
        let mut changed = installed.clone();
        match mutation {
            0 => changed.function.bounds[0].constraints.reverse(),
            1 => changed.function.bounds[0].constraints.clear(),
            2 => changed.function.bounds[0].constraints[0].arguments.clear(),
            3 => {
                changed.function.bounds[0].constraints[0]
                    .associated_types
                    .insert(output, Ty::Builtin(BuiltinType::Bool));
            }
            4 => changed.function.bounds[0].constraints[0].declaration = omega.declaration,
            5 => changed.function.generic_params.reverse(),
            6 => changed.function.params[0].mutable = true,
            7 => changed.function.return_type = Ty::Builtin(BuiltinType::Bool),
            _ => unreachable!(),
        }
        assert!(!matches(changed), "mutation {mutation}");
    }
}
