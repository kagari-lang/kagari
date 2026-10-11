use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId,
};
use kagari_contract::types::InterfaceTable;
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, Param},
    scalar::BuiltinType,
    ty::{Constraint, GenericBound, GenericParam, NominalTy, Ty},
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
        host_bridge: false,
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
            method_policy: Default::default(),
            name: "construct".into(),
            implementation: CallableImplementation::Script,
            generic_params: vec![local.clone()],
            bounds: vec![GenericBound {
                ty: source.clone(),
                constraints: vec![Constraint::Trait(NominalTy {
                    declaration: interface,
                    arguments: vec![],
                    associated_types: BTreeMap::from([(item, input.clone())]),
                })],
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
    let Constraint::Trait(bound) = &method.bounds[0].constraints[0] else {
        panic!("Iterable bound")
    };
    assert_eq!(bound.associated_types[&item], number);
}
